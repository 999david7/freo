//! Contextual feature tabs (Creo dashboards), their slide-down panels, and the dialog boxes
//! used for datum features and sketch setup.

use super::icons::{self, Icon};
use super::ribbon::{icon_button, large, small, stack, theme::*, RIB_H};
use super::*;
use crate::model::expr::Vars;
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Response, RichText, Sense, Stroke, StrokeKind};

enum Act { None, Pick(PickTarget), Sketch(Slot), SketchDialog, Ok, Cancel }

/// Numeric/expression field with Creo's white box; red text when the expression is invalid.
fn dim_field(ui: &mut egui::Ui, d: &mut Dim, vars: &Vars, width: f32) -> bool {
    let ok = d.eval(vars).is_ok();
    let mut te = egui::TextEdit::singleline(&mut d.0).desired_width(width).font(FontId::proportional(12.5));
    if !ok { te = te.text_color(Color32::from_rgb(200, 30, 30)); }
    ui.add(te).on_hover_text("Wert oder Beziehung (z. B. W/2 + 5)").changed()
}

/// Reference collector: white box, light green when active (waiting for a selection), as in Creo.
pub(crate) fn collector(ui: &mut egui::Ui, text: &str, active: bool, width: f32) -> Response {
    collector_req(ui, text, active, false, width)
}

/// Collector that also marks a missing required reference with Creo's red dot.
pub(crate) fn collector_req(ui: &mut egui::Ui, text: &str, active: bool, missing: bool, width: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
    let (bg, border) = if active { (Color32::from_rgb(230, 248, 212), Color32::from_rgb(150, 205, 120)) } else { (Color32::WHITE, BORDER) };
    ui.painter().rect_filled(rect, 2.0, bg);
    ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0_f32, if resp.hovered() { SEL_BORDER } else { border }), StrokeKind::Inside);
    let mut x = rect.left() + 6.0;
    if missing {
        ui.painter().circle_filled(pos2(x + 5.0, rect.center().y), 5.0, Color32::from_rgb(222, 34, 34));
        x += 15.0;
    }
    let col = if missing { TEXT } else if text.ends_with("wählen") || text.ends_with("(optional)") || text.starts_with("•") { Color32::from_rgb(120, 126, 134) } else { TEXT };
    let shown = if text.chars().count() > 34 { format!("{}…", text.chars().take(33).collect::<String>()) } else { text.to_string() };
    ui.painter().text(pos2(x, rect.center().y), Align2::LEFT_CENTER, shown, FontId::proportional(13.0), col);
    resp.on_hover_text("Klicken, dann im Grafikfenster oder im Modellbaum wählen")
}

/// Dashboard group as in Creo 10: bold title on top, controls below, separator on the right.
fn group<R>(ui: &mut egui::Ui, title: &str, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let out = ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
        let font = FontId::proportional(13.0);
        let tw = ui.fonts(|fo| fo.layout_no_wrap(title.to_string(), font.clone(), TEXT).size().x);
        let (tr, _) = ui.allocate_exact_size(vec2(tw + 12.0, 20.0), Sense::hover());
        let tp = pos2(tr.left() + 6.0, tr.center().y + 1.0);
        ui.painter().text(tp, Align2::LEFT_CENTER, title, font.clone(), TEXT);
        ui.painter().text(tp + vec2(0.45, 0.0), Align2::LEFT_CENTER, title, font, TEXT);
        ui.horizontal(|ui| {
            ui.set_min_height(RIB_H);
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_space(3.0);
            let r = f(ui);
            ui.add_space(3.0);
            r
        }).inner
    });
    let (sep, _) = ui.allocate_exact_size(vec2(5.0, RIB_H + 20.0), Sense::hover());
    ui.painter().line_segment([pos2(sep.center().x, sep.top() + 6.0), pos2(sep.center().x, sep.bottom() - 4.0)], Stroke::new(1.0_f32, SEP));
    out.inner
}

/// Section heading with a rule to the right ("Skizzierebene ————").
fn section_rule(ui: &mut egui::Ui, title: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).size(13.0).color(TEXT));
        let r = ui.available_rect_before_wrap();
        ui.painter().line_segment([pos2(r.left() + 4.0, r.center().y + 1.0), pos2(r.right() - 4.0, r.center().y + 1.0)], Stroke::new(1.0_f32, SEP));
    });
}

/// Default sketch orientation reference of a sketch frame: the standard datum plane facing right
/// (else up), as Creo proposes it in the Skizze dialog.
fn default_orientation(r: &crate::model::regen::RegenResult, frame: &Frame) -> Option<(String, &'static str)> {
    for (dir, label) in [(frame.x, "Rechts"), (frame.y, "Oben")] {
        if let Some(p) = r.planes.iter().filter(|p| [RIGHT_ID, TOP_ID, FRONT_ID].contains(&p.id)).find(|p| p.frame.z().dot(dir).abs() > 0.999) {
            return Some((p.name.clone(), label));
        }
    }
    None
}

fn labeled<R>(ui: &mut egui::Ui, label: &str, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(11.5).color(LABEL));
        f(ui)
    }).inner
}

/// Icon button with a ▾ that opens a menu of (icon, label, value) options.
pub(crate) fn option_menu<T: Copy + PartialEq>(ui: &mut egui::Ui, key: &str, value: &mut T, opts: &[(T, Icon, &str)]) -> bool {
    let cur = opts.iter().find(|o| o.0 == *value).copied().unwrap_or(opts[0]);
    let (rect, resp) = ui.allocate_exact_size(vec2(40.0, 26.0), Sense::click());
    if resp.hovered() { ui.painter().rect_filled(rect, 3.0, HOVER_BG); ui.painter().rect_stroke(rect, 3.0, Stroke::new(1.0_f32, HOVER_BORDER), StrokeKind::Inside); }
    icons::paint(ui.painter(), Rect::from_center_size(rect.left_center() + vec2(13.0, 0.0), vec2(20.0, 20.0)), cur.1, true);
    ui.painter().text(rect.right_center() - vec2(8.0, 0.0), Align2::CENTER_CENTER, "▾", FontId::proportional(11.0), TEXT);
    let resp = resp.on_hover_text(cur.2);
    let id = ui.id().with(key);
    if resp.clicked() { ui.memory_mut(|m| m.toggle_popup(id)); }
    let mut changed = false;
    egui::popup_below_widget(ui, id, &resp, egui::PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(150.0);
        for (v, ic, n) in opts {
            if small(ui, *ic, n, true, *v == *value).clicked() { *value = *v; changed = true; }
        }
    });
    changed
}

fn toggle_small(ui: &mut egui::Ui, icon: Icon, label: &str, value: &mut bool) -> bool {
    if small(ui, icon, label, true, *value).clicked() { *value = !*value; true } else { false }
}

impl App {
    fn ed_vars(&self) -> Vars { self.preview.as_ref().map(|p| p.vars.clone()).unwrap_or_else(|| self.result.vars.clone()) }

    fn uses_dialog(k: &FeatureKind) -> bool {
        matches!(k, FeatureKind::DatumPlane(_) | FeatureKind::DatumAxis(_) | FeatureKind::DatumPoint(_) | FeatureKind::Sketch(_) | FeatureKind::DatumCsys(_) | FeatureKind::Curve(_))
    }

    pub fn panels_for(k: &FeatureKind) -> &'static [&'static str] {
        match k {
            FeatureKind::Extrude(_) | FeatureKind::Revolve(_) => &["Platzierung", "Optionen", "Körperoptionen", "Eigenschaften"],
            FeatureKind::Sweep(_) => &["Referenzen", "Eigenschaften"],
            FeatureKind::Blend(_) => &["Schnitte", "Optionen", "Eigenschaften"],
            FeatureKind::Hole(_) => &["Platzierung", "Form", "Eigenschaften"],
            FeatureKind::Round(_) | FeatureKind::Chamfer(_) => &["Sätze", "Eigenschaften"],
            FeatureKind::Shell(_) | FeatureKind::Draft(_) | FeatureKind::Mirror(_) => &["Referenzen", "Eigenschaften"],
            FeatureKind::Pattern(_) => &["Referenzen", "Optionen", "Eigenschaften"],
            FeatureKind::Rib(_) | FeatureKind::Thread(_) => &["Referenzen", "Eigenschaften"],
            FeatureKind::Helix(_) => &["Referenzen", "Optionen", "Eigenschaften"],
            _ => &[],
        }
    }

    /// A required reference of this panel is still missing (Creo shows the panel tab in red).
    fn panel_incomplete(k: &FeatureKind, panel: &str) -> bool {
        match (panel, k) {
            ("Platzierung", FeatureKind::Extrude(d)) => d.sketch.is_empty(),
            ("Platzierung", FeatureKind::Revolve(d)) => d.sketch.is_empty(),
            _ => false,
        }
    }

    /// The contextual ribbon of the feature being created or redefined.
    pub fn feature_ribbon(&mut self, ui: &mut egui::Ui) {
        let Some(ed) = self.editor.clone() else { return };
        if Self::uses_dialog(&ed.feature.kind) {
            ui.label(RichText::new("  Das Dialogfenster ausfüllen, um den Bezug zu definieren.").color(LABEL));
            return;
        }
        let vars = self.ed_vars();
        let mut f = ed.feature.clone();
        let mut changed = false;
        let mut act = Act::None;
        let pick = ed.pick;
        let pn = |s: &App, r: &Option<PlaneRef>| s.plane_name(r);
        let left = ui.available_width();
        ui.allocate_ui_with_layout(vec2(left, super::ribbon::RIB_H + 22.0), egui::Layout::left_to_right(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            match &mut f.kind {
                FeatureKind::Extrude(d) => {
                    // Creo's Extrudieren tab: feature icon, Typ, Tiefe, Einstellungen; the section is
                    // defined from the Platzierung panel or by picking a plane / sketch directly
                    let (ir, _) = ui.allocate_exact_size(vec2(62.0, RIB_H + 20.0), Sense::hover());
                    icons::paint(ui.painter(), Rect::from_center_size(pos2(ir.center().x, ir.top() + 52.0), vec2(40.0, 40.0)), Icon::Extrude, true);
                    let (sep, _) = ui.allocate_exact_size(vec2(5.0, RIB_H + 20.0), Sense::hover());
                    ui.painter().line_segment([pos2(sep.center().x, sep.top() + 6.0), pos2(sep.center().x, sep.bottom() - 4.0)], Stroke::new(1.0_f32, SEP));
                    let has = !d.sketch.is_empty();
                    group(ui, "Typ", |ui| {
                        large(ui, Icon::Body, "Volumen", has, true).on_hover_text("Ein Volumen-Profil erzeugen");
                        large(ui, Icon::FillSurf, "Fläche", false, false).on_hover_text("Flächen-Profile werden in Freo nicht unterstützt");
                    });
                    group(ui, "Tiefe", |ui| {
                        stack(ui, |ui| {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                changed |= option_menu(ui, "depth", &mut d.depth_type, &[(DepthType::Blind, Icon::DepthBlind, "Variabel"), (DepthType::Symmetric, Icon::DepthSymmetric, "Symmetrisch"), (DepthType::ToNext, Icon::DepthNext, "Bis zur nächsten"), (DepthType::ThroughAll, Icon::DepthThrough, "Durch alle"), (DepthType::ToSelected, Icon::DepthSelected, "Bis zur ausgewählten")]);
                                match d.depth_type {
                                    DepthType::Blind | DepthType::Symmetric => { changed |= dim_field(ui, &mut d.depth, &vars, 90.0); }
                                    DepthType::ToSelected => { if collector(ui, &pn(self, &d.to_ref), pick == Some(PickTarget::DepthRef), 90.0).clicked() { act = Act::Pick(PickTarget::DepthRef); } }
                                    _ => {}
                                }
                                if icon_button(ui, Icon::Flip, "Profil in die andere Richtung der Skizze erzeugen", true, d.flip, 18.0).clicked() { d.flip = !d.flip; changed = true; }
                            });
                            ui.add_space(6.0);
                            small(ui, Icon::Shell, "Geschlossene Enden", false, false).on_hover_text("Nur für Flächen-Profile");
                        });
                    });
                    group(ui, "Einstellungen", |ui| {
                        stack(ui, |ui| {
                            changed |= toggle_small(ui, Icon::RemoveMaterial, "Material entfernen", &mut d.remove);
                            changed |= toggle_small(ui, Icon::Thicken, "Skizze aufdicken", &mut d.thin);
                            if d.thin { ui.horizontal(|ui| { ui.add_space(22.0); changed |= dim_field(ui, &mut d.thickness, &vars, 56.0); }); }
                        });
                    });
                }
                FeatureKind::Revolve(d) => {
                    group(ui, "Winkel", |ui| {
                        stack(ui, |ui| {
                            ui.horizontal(|ui| {
                                let mut sym = d.symmetric;
                                if option_menu(ui, "revang", &mut sym, &[(false, Icon::DepthBlind, "Variabel"), (true, Icon::DepthSymmetric, "Symmetrisch")]) { d.symmetric = sym; changed = true; }
                                changed |= dim_field(ui, &mut d.angle, &vars, 62.0);
                                if icon_button(ui, Icon::Flip, "Winkelrichtung ändern", true, d.flip, 18.0).clicked() { d.flip = !d.flip; changed = true; }
                            });
                            ui.label(RichText::new("Achse: interne Mittellinie").size(11.0).color(LABEL));
                        });
                    });
                    group(ui, "Einstellungen", |ui| { stack(ui, |ui| { changed |= toggle_small(ui, Icon::RemoveMaterial, "Material entfernen", &mut d.remove); }); });
                    group(ui, "Skizze", |ui| {
                        if large(ui, Icon::Sketch, if d.sketch.is_empty() { "Definieren" } else { "Skizze\neditieren" }, true, false).clicked() { act = Act::Sketch(Slot::Main); }
                    });
                }
                FeatureKind::Sweep(d) => {
                    group(ui, "Leitkurve", |ui| {
                        if large(ui, Icon::Sketch, if d.trajectory.is_empty() { "Leitkurve\nskizzieren" } else { "Leitkurve\neditieren" }, true, false).clicked() { act = Act::Sketch(Slot::Trajectory); }
                    });
                    group(ui, "Schnitt", |ui| {
                        if large(ui, Icon::SkRect, if d.section.is_empty() { "Querschnitt\nskizzieren" } else { "Querschnitt\neditieren" }, !d.trajectory.is_empty(), false).on_hover_text("Den Zugquerschnitt erzeugen oder editieren").clicked() { act = Act::Sketch(Slot::Section); }
                    });
                    group(ui, "Einstellungen", |ui| { stack(ui, |ui| { changed |= toggle_small(ui, Icon::RemoveMaterial, "Material entfernen", &mut d.remove); }); });
                }
                FeatureKind::Blend(d) => {
                    group(ui, "Schnitte", |ui| {
                        stack(ui, |ui| {
                            for i in 0..d.sections.len().min(3) {
                                if small(ui, Icon::Sketch, &format!("Querschnitt {}", i + 1), true, false).clicked() { act = Act::Sketch(Slot::Blend(i)); }
                            }
                        });
                        if large(ui, Icon::Blend, "Querschnitt\neinfügen", true, false).clicked() { act = Act::Sketch(Slot::Blend(d.sections.len())); }
                    });
                    group(ui, if d.rotational { "Winkel" } else { "Versätze" }, |ui| {
                        stack(ui, |ui| {
                            for i in 0..d.sections.len().saturating_sub(1).min(3) {
                                while d.offsets.len() <= i { d.offsets.push(Dim::v(if d.rotational { 45.0 } else { 20.0 })); }
                                labeled(ui, &format!("{}→{}", i + 1, i + 2), |ui| { changed |= dim_field(ui, &mut d.offsets[i], &vars, 56.0); });
                            }
                        });
                    });
                    group(ui, "Einstellungen", |ui| {
                        stack(ui, |ui| {
                            changed |= toggle_small(ui, Icon::RemoveMaterial, "Material entfernen", &mut d.remove);
                            changed |= toggle_small(ui, Icon::Flip, "Richtung umkehren", &mut d.flip);
                        });
                    });
                }
                FeatureKind::Hole(d) => {
                    group(ui, "Typ", |ui| {
                        for (t, ic, n) in [(HoleType::Simple, Icon::HoleSimple, "Einfach"), (HoleType::Standard, Icon::HoleStandard, "Norm"), (HoleType::Counterbore, Icon::HoleCbore, "Zylindersenkung"), (HoleType::Countersink, Icon::HoleCsink, "Kegelsenkung")] {
                            if large(ui, ic, n, true, d.ty == t).clicked() { d.ty = t; changed = true; }
                        }
                    });
                    if d.ty == HoleType::Standard {
                        group(ui, "Gewinde", |ui| {
                            stack(ui, |ui| {
                                ui.horizontal(|ui| {
                                    egui::ComboBox::from_id_salt("thstd").width(56.0).selected_text(match d.standard { ThreadStd::Iso => "ISO", ThreadStd::Unc => "UNC" }).show_ui(ui, |ui| {
                                        for (st, n) in [(ThreadStd::Iso, "ISO"), (ThreadStd::Unc, "UNC")] {
                                            if ui.selectable_value(&mut d.standard, st, n).changed() { d.size = crate::model::threads::table(st)[if st == ThreadStd::Iso { 5 } else { 4 }].name.to_string(); changed = true; }
                                        }
                                    });
                                    egui::ComboBox::from_id_salt("thsize").width(70.0).selected_text(d.size.clone()).show_ui(ui, |ui| {
                                        for t in crate::model::threads::table(d.standard) { if ui.selectable_label(d.size == t.name, t.name).clicked() { d.size = t.name.to_string(); changed = true; } }
                                    });
                                });
                                ui.horizontal(|ui| {
                                    if ui.selectable_label(d.tapped, "Gewinde").clicked() { d.tapped = true; changed = true; }
                                    if ui.selectable_label(!d.tapped, "Durchgangsloch").clicked() { d.tapped = false; changed = true; }
                                });
                                if d.tapped {
                                    labeled(ui, "Gewindetiefe", |ui| { if !d.through_all { changed |= dim_field(ui, &mut d.thread_depth, &vars, 44.0); } else { ui.label("voll"); } });
                                } else {
                                    egui::ComboBox::from_id_salt("thfit").width(80.0).selected_text(format!("Passung {}", d.fit.label())).show_ui(ui, |ui| {
                                        for f2 in [HoleFit::Close, HoleFit::Medium, HoleFit::Free] { changed |= ui.selectable_value(&mut d.fit, f2, f2.label()).changed(); }
                                    });
                                }
                            });
                        });
                    }
                    group(ui, "Größe", |ui| {
                        stack(ui, |ui| {
                            if d.ty == HoleType::Standard {
                                let dd = crate::model::regen::hole_drill(d, &vars).unwrap_or(0.0);
                                ui.label(RichText::new(format!("Bohrer ⌀ {}", fmt_num(dd, 3))).size(11.5).color(LABEL));
                            } else {
                                labeled(ui, "⌀", |ui| { changed |= dim_field(ui, &mut d.diameter, &vars, 60.0); });
                            }
                            ui.horizontal(|ui| {
                                let mut ta = d.through_all;
                                if option_menu(ui, "holedepth", &mut ta, &[(false, Icon::DepthBlind, "Variabel"), (true, Icon::DepthThrough, "Durch alle")]) { d.through_all = ta; changed = true; }
                                if !d.through_all { changed |= dim_field(ui, &mut d.depth, &vars, 56.0); }
                            });
                            if !d.through_all { changed |= toggle_small(ui, Icon::HoleSimple, "Bohrspitze", &mut d.drill_tip); }
                        });
                    });
                    match d.ty {
                        HoleType::Counterbore => group(ui, "Zylindersenkung", |ui| { stack(ui, |ui| {
                            labeled(ui, "⌀", |ui| { changed |= dim_field(ui, &mut d.cb_diameter, &vars, 56.0); });
                            labeled(ui, "Tiefe", |ui| { changed |= dim_field(ui, &mut d.cb_depth, &vars, 56.0); });
                        }); }),
                        HoleType::Countersink => group(ui, "Kegelsenkung", |ui| { stack(ui, |ui| {
                            labeled(ui, "⌀", |ui| { changed |= dim_field(ui, &mut d.cs_diameter, &vars, 56.0); });
                            labeled(ui, "Winkel", |ui| { changed |= dim_field(ui, &mut d.cs_angle, &vars, 56.0); });
                        }); }),
                        HoleType::Simple | HoleType::Standard => {}
                    }
                    group(ui, "Platzierung", |ui| {
                        stack(ui, |ui| {
                            if collector(ui, &pn(self, &d.plane), pick == Some(PickTarget::Plane), 150.0).clicked() { act = Act::Pick(PickTarget::Plane); }
                            ui.horizontal(|ui| {
                                labeled(ui, "X", |ui| { changed |= dim_field(ui, &mut d.x, &vars, 44.0); });
                                labeled(ui, "Y", |ui| { changed |= dim_field(ui, &mut d.y, &vars, 44.0); });
                            });
                            changed |= toggle_small(ui, Icon::Flip, "Richtung umkehren", &mut d.flip);
                        });
                    });
                }
                FeatureKind::Round(d) => {
                    let nsets = d.sets.len();
                    let mut active = self.round_set.min(nsets - 1);
                    group(ui, "Sätze", |ui| {
                        stack(ui, |ui| {
                            ui.horizontal(|ui| {
                                egui::ComboBox::from_id_salt("roundset").width(70.0).selected_text(format!("Satz {}", active + 1)).show_ui(ui, |ui| {
                                    for i in 0..nsets { ui.selectable_value(&mut active, i, format!("Satz {}", i + 1)); }
                                });
                                if ui.small_button("＋ Neuer Satz").on_hover_text("Einen neuen Rundungssatz beginnen").clicked() {
                                    let r = d.sets[active].radius.clone();
                                    d.sets.push(RoundSet { edges: vec![], radius: r.clone(), variable: false, radius_end: r });
                                    active = d.sets.len() - 1;
                                    act = Act::Pick(PickTarget::Edges);
                                    changed = true;
                                }
                            });
                            let set = &mut d.sets[active];
                            if collector(ui, &if set.edges.is_empty() { "Kanten wählen".to_string() } else { format!("{} Kante(n)", set.edges.len()) }, pick == Some(PickTarget::Edges), 150.0).clicked() { act = Act::Pick(PickTarget::Edges); }
                        });
                    });
                    group(ui, "Radius", |ui| {
                        stack(ui, |ui| {
                            let set = &mut d.sets[active];
                            labeled(ui, if set.variable { "Start" } else { "Radius" }, |ui| { changed |= dim_field(ui, &mut set.radius, &vars, 60.0); });
                            changed |= toggle_small(ui, Icon::Round, "Variabel", &mut set.variable);
                            if set.variable { labeled(ui, "Ende", |ui| { changed |= dim_field(ui, &mut set.radius_end, &vars, 60.0); }); }
                        });
                    });
                    self.round_set = active;
                }
                FeatureKind::Chamfer(d) => {
                    group(ui, "Bemaßung", |ui| {
                        stack(ui, |ui| {
                            egui::ComboBox::from_id_salt("chscheme").width(100.0).selected_text(match d.kind { ChamferKind::DxD => "D x D", ChamferKind::D1xD2 => "D1 x D2", ChamferKind::AnglexD => "Winkel x D" }).show_ui(ui, |ui| {
                                changed |= ui.selectable_value(&mut d.kind, ChamferKind::DxD, "D x D").changed();
                                changed |= ui.selectable_value(&mut d.kind, ChamferKind::D1xD2, "D1 x D2").changed();
                                changed |= ui.selectable_value(&mut d.kind, ChamferKind::AnglexD, "Winkel x D").changed();
                            });
                            ui.horizontal(|ui| {
                                labeled(ui, "D", |ui| { changed |= dim_field(ui, &mut d.d1, &vars, 48.0); });
                                if d.kind == ChamferKind::D1xD2 { labeled(ui, "D2", |ui| { changed |= dim_field(ui, &mut d.d2, &vars, 48.0); }); }
                                if d.kind == ChamferKind::AnglexD { labeled(ui, "∠", |ui| { changed |= dim_field(ui, &mut d.angle, &vars, 48.0); }); }
                            });
                        });
                    });
                    group(ui, "Sätze", |ui| {
                        if collector(ui, &if d.edges.is_empty() { "Kanten wählen".to_string() } else { format!("{} Kante(n)", d.edges.len()) }, pick == Some(PickTarget::Edges), 150.0).clicked() { act = Act::Pick(PickTarget::Edges); }
                    });
                }
                FeatureKind::Shell(d) => {
                    group(ui, "Einstellungen", |ui| { stack(ui, |ui| { labeled(ui, "Dicke", |ui| { changed |= dim_field(ui, &mut d.thickness, &vars, 64.0); }); }); });
                    group(ui, "Entfernte Flächen", |ui| {
                        if collector(ui, &if d.faces.is_empty() { "Flächen wählen (optional)".to_string() } else { format!("{} Fläche(n)", d.faces.len()) }, pick == Some(PickTarget::Faces), 170.0).clicked() { act = Act::Pick(PickTarget::Faces); }
                    });
                }
                FeatureKind::Draft(d) => {
                    group(ui, "Referenzen", |ui| {
                        stack(ui, |ui| {
                            labeled(ui, "Flächen", |ui| { if collector(ui, &if d.faces.is_empty() { "Flächen wählen".to_string() } else { format!("{} Fläche(n)", d.faces.len()) }, pick == Some(PickTarget::Faces), 140.0).clicked() { act = Act::Pick(PickTarget::Faces); } });
                            labeled(ui, "Scharnier", |ui| { if collector(ui, &pn(self, &d.hinge), pick == Some(PickTarget::Plane), 140.0).clicked() { act = Act::Pick(PickTarget::Plane); } });
                        });
                    });
                    group(ui, "Winkel", |ui| {
                        stack(ui, |ui| {
                            ui.horizontal(|ui| { changed |= dim_field(ui, &mut d.angle, &vars, 56.0); if icon_button(ui, Icon::Flip, "Formschrägenrichtung umkehren", true, d.flip, 18.0).clicked() { d.flip = !d.flip; changed = true; } });
                        });
                    });
                }
                FeatureKind::Pattern(d) => {
                    group(ui, "Typ", |ui| {
                        if large(ui, Icon::Pattern, "Richtung", true, d.kind == PatternKind::Direction).clicked() { d.kind = PatternKind::Direction; changed = true; }
                        if large(ui, Icon::Revolve, "Achse", true, d.kind == PatternKind::Axis).clicked() { d.kind = PatternKind::Axis; changed = true; }
                        if large(ui, Icon::PatternFill, "Füllen", true, d.kind == PatternKind::Fill).clicked() { d.kind = PatternKind::Fill; changed = true; }
                        if large(ui, Icon::PatternCurve, "Kurve", true, d.kind == PatternKind::Curve).clicked() { d.kind = PatternKind::Curve; changed = true; }
                    });
                    match d.kind {
                        PatternKind::Direction => {
                            group(ui, "Erste Richtung", |ui| {
                                stack(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        egui::ComboBox::from_id_salt("pd1").width(80.0).selected_text(self.axis_name(&d.dir1)).show_ui(ui, |ui| {
                                            for a in [AxisRef::WorldX, AxisRef::WorldY, AxisRef::WorldZ] { changed |= ui.selectable_value(&mut d.dir1, a, self.axis_name(&a)).changed(); }
                                        });
                                        if ui.small_button("Wählen").on_hover_text("Eine Achse oder eine Standardbezugsebene wählen").clicked() { act = Act::Pick(PickTarget::Direction); }
                                    });
                                    labeled(ui, "Elemente", |ui| { changed |= dim_field(ui, &mut d.count1, &vars, 40.0); });
                                    labeled(ui, "Abstand", |ui| { changed |= dim_field(ui, &mut d.spacing1, &vars, 48.0); });
                                });
                            });
                            group(ui, "Zweite Richtung", |ui| {
                                stack(ui, |ui| {
                                    changed |= ui.checkbox(&mut d.use_dir2, "Verwenden").changed();
                                    if d.use_dir2 {
                                        egui::ComboBox::from_id_salt("pd2").width(80.0).selected_text(self.axis_name(&d.dir2)).show_ui(ui, |ui| {
                                            for a in [AxisRef::WorldX, AxisRef::WorldY, AxisRef::WorldZ] { changed |= ui.selectable_value(&mut d.dir2, a, self.axis_name(&a)).changed(); }
                                        });
                                        ui.horizontal(|ui| { changed |= dim_field(ui, &mut d.count2, &vars, 36.0); changed |= dim_field(ui, &mut d.spacing2, &vars, 44.0); });
                                    }
                                });
                            });
                        }
                        PatternKind::Axis => {
                            group(ui, "Achse", |ui| {
                                stack(ui, |ui| {
                                    if collector(ui, &self.axis_name(&d.axis), pick == Some(PickTarget::Axis), 120.0).clicked() { act = Act::Pick(PickTarget::Axis); }
                                    labeled(ui, "Elemente", |ui| { changed |= dim_field(ui, &mut d.axis_count, &vars, 40.0); });
                                    ui.horizontal(|ui| {
                                        labeled(ui, "Winkel", |ui| { changed |= dim_field(ui, &mut d.axis_angle, &vars, 44.0); });
                                        if ui.small_button("360°").clicked() { let n = d.axis_count.value(&vars, 1.0).max(1.0); d.axis_angle = Dim::v(360.0 / n); changed = true; }
                                    });
                                });
                            });
                        }
                        PatternKind::Fill | PatternKind::Curve => {
                            let fill = d.kind == PatternKind::Fill;
                            group(ui, if fill { "Füllbereich" } else { "Kurve" }, |ui| {
                                stack(ui, |ui| {
                                    labeled(ui, "Ebene", |ui| { if collector(ui, &pn(self, &d.sketch_plane), pick == Some(PickTarget::SketchPlane), 110.0).clicked() { act = Act::Pick(PickTarget::SketchPlane); } });
                                    if ui.button(if d.sketch.is_empty() { "✎ Skizze definieren..." } else { "✎ Skizze editieren..." }).clicked() { act = if d.sketch_plane.is_some() { Act::Sketch(Slot::Pattern) } else { Act::Pick(PickTarget::SketchPlane) }; }
                                });
                            });
                            if fill {
                                group(ui, "Raster", |ui| {
                                    stack(ui, |ui| {
                                        egui::ComboBox::from_id_salt("fillgrid").width(90.0).selected_text(d.fill_grid.label()).show_ui(ui, |ui| {
                                            for g in [FillGrid::Square, FillGrid::Diamond, FillGrid::Triangle] { changed |= ui.selectable_value(&mut d.fill_grid, g, g.label()).changed(); }
                                        });
                                        labeled(ui, "Abstand", |ui| { changed |= dim_field(ui, &mut d.fill_spacing, &vars, 44.0); });
                                        ui.horizontal(|ui| {
                                            labeled(ui, "Rand", |ui| { changed |= dim_field(ui, &mut d.fill_margin, &vars, 36.0); });
                                            labeled(ui, "∠", |ui| { changed |= dim_field(ui, &mut d.fill_rotation, &vars, 36.0); });
                                        });
                                    });
                                });
                            } else {
                                group(ui, "Abstand", |ui| {
                                    stack(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            if ui.selectable_label(!d.curve_by_spacing, "Elemente").clicked() { d.curve_by_spacing = false; changed = true; }
                                            if ui.selectable_label(d.curve_by_spacing, "Abstand").clicked() { d.curve_by_spacing = true; changed = true; }
                                        });
                                        if d.curve_by_spacing { changed |= dim_field(ui, &mut d.curve_spacing, &vars, 56.0); } else { changed |= dim_field(ui, &mut d.curve_count, &vars, 56.0); }
                                        changed |= ui.checkbox(&mut d.curve_follow, "Kurvenrichtung folgen").changed();
                                    });
                                });
                            }
                        }
                    }
                }
                FeatureKind::Rib(d) => {
                    group(ui, "Dicke", |ui| {
                        stack(ui, |ui| {
                            ui.horizontal(|ui| {
                                changed |= dim_field(ui, &mut d.thickness, &vars, 64.0);
                                let side = ["Symmetrisch", "Seite 1", "Seite 2"][d.side.min(2) as usize];
                                if icon_button(ui, Icon::DepthSymmetric, &format!("Dickenseite wechseln ({})", side), true, d.side != 0, 18.0).clicked() { d.side = (d.side + 1) % 3; changed = true; }
                                if icon_button(ui, Icon::Flip, "Materialseite umkehren", true, d.flip, 18.0).clicked() { d.flip = !d.flip; changed = true; }
                            });
                            ui.label(RichText::new(format!("Dicke: {}   •   Skizzierebene: {}", ["symmetrisch", "Seite 1", "Seite 2"][d.side.min(2) as usize], pn(self, &d.plane))).size(11.0).color(LABEL));
                        });
                    });
                    group(ui, "Skizze", |ui| {
                        if large(ui, Icon::Sketch, if d.sketch.is_empty() { "Definieren" } else { "Skizze\neditieren" }, true, false).on_hover_text("Eine offene Kette skizzieren, deren Enden das Volumen berühren").clicked() { act = Act::Sketch(Slot::Main); }
                    });
                }
                FeatureKind::Helix(d) => {
                    group(ui, "Profil", |ui| {
                        if large(ui, Icon::Sketch, if d.profile.is_empty() { "Profil\nskizzieren" } else { "Profil\neditieren" }, true, false).on_hover_text("Mittellinie (Achse) und offene Profilkette skizzieren").clicked() { act = Act::Sketch(Slot::Trajectory); }
                    });
                    group(ui, "Schnitt", |ui| {
                        if large(ui, Icon::SkRect, if d.section.is_empty() { "Querschnitt\nskizzieren" } else { "Querschnitt\neditieren" }, !d.profile.is_empty(), false).clicked() { act = Act::Sketch(Slot::Section); }
                    });
                    group(ui, "Steigung", |ui| {
                        stack(ui, |ui| {
                            labeled(ui, "Steigung", |ui| { changed |= dim_field(ui, &mut d.pitch, &vars, 56.0); });
                            ui.horizontal(|ui| {
                                if ui.selectable_label(!d.left_handed, "Rechtsgängig").clicked() { d.left_handed = false; changed = true; }
                                if ui.selectable_label(d.left_handed, "Linksgängig").clicked() { d.left_handed = true; changed = true; }
                            });
                        });
                    });
                    group(ui, "Einstellungen", |ui| { stack(ui, |ui| { changed |= toggle_small(ui, Icon::RemoveMaterial, "Material entfernen", &mut d.remove); }); });
                }
                FeatureKind::Thread(d) => {
                    group(ui, "Referenzen", |ui| {
                        stack(ui, |ui| {
                            labeled(ui, "Fläche", |ui| { if collector(ui, if d.face.is_some() { "Zylinderfläche" } else { "Fläche wählen" }, pick == Some(PickTarget::Surface), 120.0).clicked() { act = Act::Pick(PickTarget::Surface); } });
                            changed |= toggle_small(ui, Icon::Flip, "Am anderen Ende beginnen", &mut d.flip);
                        });
                    });
                    group(ui, "Gewinde", |ui| {
                        stack(ui, |ui| {
                            ui.horizontal(|ui| {
                                labeled(ui, "⌀", |ui| { changed |= dim_field(ui, &mut d.diameter, &vars, 48.0); });
                                labeled(ui, "Steigung", |ui| { changed |= dim_field(ui, &mut d.pitch, &vars, 44.0); });
                            });
                            labeled(ui, "Länge", |ui| { changed |= dim_field(ui, &mut d.length, &vars, 56.0); });
                            let label = crate::model::regen::thread_label(d.diameter.value(&vars, 0.0), d.pitch.value(&vars, 0.0));
                            ui.label(RichText::new(format!("Bezeichnung: {}", label)).size(11.0).color(LABEL));
                        });
                    });
                }
                FeatureKind::Mirror(d) => {
                    group(ui, "Spiegelebene", |ui| {
                        if collector(ui, &pn(self, &d.plane), pick == Some(PickTarget::Plane), 150.0).clicked() { act = Act::Pick(PickTarget::Plane); }
                    });
                    group(ui, "KEs", |ui| {
                        stack(ui, |ui| {
                            let names: Vec<String> = d.features.iter().filter_map(|id| self.data.feature(*id).map(|f| f.name.clone())).collect();
                            if collector(ui, &if names.is_empty() { "Gesamte Geometrie".into() } else { names.join(", ") }, pick == Some(PickTarget::Features), 160.0).clicked() { act = Act::Pick(PickTarget::Features); }
                            if !d.features.is_empty() && ui.small_button("Gesamte Geometrie spiegeln").clicked() { d.features.clear(); changed = true; }
                        });
                    });
                }
                _ => {}
            }
            // right-hand controls: preview, OK, cancel
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                ui.add_space(6.0);
                if let Some(help) = feature_help(&ed.feature.kind) {
                    // Creo 10's description box at the right end of the dashboard
                    egui::Frame::NONE.fill(Color32::from_rgb(236, 238, 240)).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                        ui.set_width(230.0);
                        ui.set_min_height(RIB_H);
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| { ui.add(egui::Label::new(RichText::new(help).size(12.5).color(TEXT)).wrap()); });
                    });
                    ui.add_space(8.0);
                }
                if large(ui, Icon::Cancel, "Abbrechen", true, false).on_hover_text("Abbrechen (Esc)").clicked() { act = Act::Cancel; }
                let err = self.editor_error();
                // a missing section is shown by the red Platzierung tab, as in Creo, not as an error text
                let shown_err = err.clone().filter(|_| !Self::panel_incomplete(&ed.feature.kind, "Platzierung"));
                if large(ui, Icon::Ok, "OK", err.is_none(), false).on_hover_text(err.clone().unwrap_or_else(|| "Änderungen anwenden und speichern (Enter)".into())).clicked() { act = Act::Ok; }
                let mut prev = ed.preview;
                if large(ui, Icon::Preview, "Vorschau", true, prev).on_hover_text("Geometrievorschau").clicked() { prev = !prev; if let Some(e) = &mut self.editor { e.preview = prev; } }
                if let Some(e) = shown_err {
                    ui.add_space(8.0);
                    ui.add(egui::Label::new(RichText::new(format!("⚠ {}", e)).color(Color32::from_rgb(190, 40, 30)).size(11.5)).wrap());
                }
            });
        });
        if f != ed.feature { let nf = f; self.update_editor(|x| *x = nf); }
        self.run_act(act);
    }

    fn run_act(&mut self, act: Act) {
        match act {
            Act::None => {}
            Act::Pick(t) => self.set_pick_target(Some(t)),
            Act::Sketch(s) => self.begin_sketch(s),
            Act::SketchDialog => self.open_sketch_dialog(),
            Act::Ok => self.finish_feature(),
            Act::Cancel => self.cancel_feature(),
        }
    }

    /// The row of slide-down panel tabs ("Placement  Options  Properties") under the dashboard.
    pub fn feature_panel_tabs(&mut self, ui: &mut egui::Ui) {
        let Some(ed) = &self.editor else { return };
        let panels = Self::panels_for(&ed.feature.kind);
        if panels.is_empty() { return; }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_space(4.0);
            for p in panels {
                let font = FontId::proportional(13.5);
                let w = ui.fonts(|f| f.layout_no_wrap(p.to_string(), font.clone(), Color32::BLACK).size().x) + 26.0;
                let (rect, resp) = ui.allocate_exact_size(vec2(w, 27.0), Sense::click());
                let active = self.dash_panel == Some(*p);
                let missing = Self::panel_incomplete(&ed.feature.kind, p);
                if active { self.dash_panel_anchor = rect.left_bottom(); }
                let painter = ui.painter();
                if active {
                    painter.rect_filled(rect, egui::CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 }, Color32::WHITE);
                    painter.rect_stroke(rect, egui::CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 }, Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
                } else if resp.hovered() {
                    painter.rect_filled(rect, 3.0, HOVER_BG);
                }
                painter.text(rect.center(), Align2::CENTER_CENTER, *p, font, if missing { Color32::from_rgb(200, 30, 30) } else if active { TEXT } else { Color32::from_rgb(40, 90, 170) });
                if resp.clicked() {
                    self.dash_panel = if active { None } else { Some(*p) };
                    self.dash_panel_anchor = rect.left_bottom();
                }
            }
        });
    }

    /// Slide-down panel content for the active panel tab.
    pub fn feature_panel_ui(&mut self, ctx: &egui::Context) {
        let (Some(ed), Some(panel)) = (self.editor.clone(), self.dash_panel) else { return };
        let vars = self.ed_vars();
        let mut f = ed.feature.clone();
        let mut act = Act::None;
        let pick = ed.pick;
        let mut close = false;
        let missing = Self::panel_incomplete(&ed.feature.kind, panel);
        egui::Area::new(egui::Id::new("dash_panel")).fixed_pos(self.dash_panel_anchor).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(Color32::WHITE).inner_margin(0.0).show(ui, |ui| {
                // Creo's panel title bar: centred title, pink while a required reference is missing
                let (bar, _) = ui.allocate_exact_size(vec2(340.0, 30.0), Sense::hover());
                ui.painter().rect_filled(bar, egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, if missing { Color32::from_rgb(255, 224, 226) } else { Color32::from_rgb(236, 240, 244) });
                ui.painter().text(bar.center(), Align2::CENTER_CENTER, panel, FontId::proportional(14.0), Color32::from_rgb(120, 126, 134));
                let xr = Rect::from_center_size(pos2(bar.right() - 16.0, bar.center().y), vec2(22.0, 22.0));
                let xresp = ui.interact(xr, ui.id().with("dash_close"), Sense::click()).on_hover_text("Schließen");
                let xc = if xresp.hovered() { Color32::from_rgb(214, 48, 40) } else { Color32::from_rgb(110, 116, 124) };
                for d in [vec2(4.5, 4.5), vec2(4.5, -4.5)] { ui.painter().line_segment([xr.center() - d, xr.center() + d], Stroke::new(1.8_f32, xc)); }
                if xresp.clicked() { close = true; }
                egui::Frame::NONE.inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 12 }).show(ui, |ui| {
                ui.set_width(316.0);
                let pn = |s: &App, r: &Option<PlaneRef>| s.plane_name(r);
                if panel == "Eigenschaften" {
                    labeled(ui, "Name", |ui| { ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(200.0)); });
                    ui.label(RichText::new(format!("KE-Typ: {}   •   ID {}", ed.feature.kind.type_name(), ed.feature.id)).size(11.0).color(LABEL));
                }
                match (panel, &mut f.kind) {
                    ("Platzierung", FeatureKind::Extrude(ExtrudeDef { sketch, .. })) | ("Platzierung", FeatureKind::Revolve(RevolveDef { sketch, .. })) => {
                        ui.label(RichText::new("Skizze").size(13.5));
                        ui.add_space(2.0);
                        ui.horizontal(|ui| {
                            let empty = sketch.is_empty();
                            let name = if empty { "1 Element auswählen" } else { "Querschnitt 1 (intern)" };
                            let active = pick == Some(PickTarget::SketchPlane) && self.sketch_dlg.is_none();
                            if collector_req(ui, name, active, empty, 200.0).on_hover_text("Eine Ebene, eine ebene Fläche oder eine vorhandene Skizze wählen").clicked() { act = Act::Pick(PickTarget::SketchPlane); }
                            if ui.add(egui::Button::new(RichText::new(if empty { "Definieren..." } else { "Editieren..." }).size(13.5)).min_size(vec2(100.0, 26.0))).clicked() {
                                act = if empty { Act::SketchDialog } else { Act::Sketch(Slot::Main) };
                            }
                        });
                    }
                    ("Körperoptionen", FeatureKind::Extrude(_) | FeatureKind::Revolve(_)) => {
                        ui.add_enabled(false, egui::Checkbox::new(&mut false, "Neuen Körper erzeugen"));
                        labeled(ui, "Körper", |ui| { ui.label("Körper 1 (Standardkörper)"); });
                        ui.label(RichText::new("Freo modelliert jedes Teil als einen Körper.").size(11.0).color(LABEL));
                    }
                    ("Optionen", FeatureKind::Extrude(d)) => {
                        egui::Grid::new("extopts").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                            ui.label("Seite 1");
                            ui.horizontal(|ui| {
                                egui::ComboBox::from_id_salt("s1").width(100.0).selected_text(d.depth_type.label()).show_ui(ui, |ui| {
                                    for (t, n) in [(DepthType::Blind, "Variabel"), (DepthType::Symmetric, "Symmetrisch"), (DepthType::ToNext, "Bis zur nächsten"), (DepthType::ThroughAll, "Durch alle"), (DepthType::ToSelected, "Bis zur ausgewählten")] { ui.selectable_value(&mut d.depth_type, t, n); }
                                });
                                if matches!(d.depth_type, DepthType::Blind | DepthType::Symmetric) { dim_field(ui, &mut d.depth, &vars, 60.0); }
                            });
                            ui.end_row();
                            ui.label("Seite 2");
                            ui.horizontal(|ui| {
                                ui.add_enabled_ui(d.depth_type != DepthType::Symmetric, |ui| {
                                    egui::ComboBox::from_id_salt("s2").width(100.0).selected_text(d.side2.label()).show_ui(ui, |ui| {
                                        for t in [Side2::None, Side2::Blind, Side2::ThroughAll] { ui.selectable_value(&mut d.side2, t, t.label()); }
                                    });
                                    if d.side2 == Side2::Blind { dim_field(ui, &mut d.depth2, &vars, 60.0); }
                                });
                            });
                            ui.end_row();
                            ui.label("Verjüngung hinzufügen");
                            ui.horizontal(|ui| { dim_field(ui, &mut d.taper, &vars, 60.0); ui.label("°"); });
                            ui.end_row();
                        });
                    }
                    ("Optionen", FeatureKind::Revolve(d)) => {
                        ui.checkbox(&mut d.symmetric, "Symmetrisch zur Skizzierebene");
                        labeled(ui, "Winkel", |ui| { dim_field(ui, &mut d.angle, &vars, 60.0); });
                    }
                    ("Referenzen", FeatureKind::Sweep(d)) => {
                        labeled(ui, "Leitkurvenebene", |ui| { if collector(ui, &pn(self, &d.plane), pick == Some(PickTarget::SketchPlane), 160.0).clicked() { act = Act::Pick(PickTarget::SketchPlane); } });
                        ui.horizontal(|ui| {
                            if ui.button("Leitkurve editieren...").clicked() { act = Act::Sketch(Slot::Trajectory); }
                            if ui.button("Querschnitt editieren...").clicked() { act = Act::Sketch(Slot::Section); }
                        });
                        ui.label(RichText::new("Der Querschnitt wird senkrecht zum Anfang der Leitkurve skizziert.").size(11.0).color(LABEL));
                    }
                    ("Schnitte", FeatureKind::Blend(d)) => {
                        labeled(ui, "Skizzierebene", |ui| { if collector(ui, &pn(self, &d.plane), pick == Some(PickTarget::SketchPlane), 160.0).clicked() { act = Act::Pick(PickTarget::SketchPlane); } });
                        egui::Grid::new("blendsec").striped(true).show(ui, |ui| {
                            ui.label(RichText::new("#").strong()); ui.label(RichText::new("Schnitt").strong()); ui.label(RichText::new(if d.rotational { "Winkel zum vorherigen (°)" } else { "Versatz zum vorherigen" }).strong()); ui.end_row();
                            for i in 0..d.sections.len() {
                                ui.label(format!("{}", i + 1));
                                if ui.button(if d.sections[i].is_empty() { "Skizze..." } else { "Editieren..." }).clicked() { act = Act::Sketch(Slot::Blend(i)); }
                                if i > 0 {
                                    while d.offsets.len() < i { d.offsets.push(Dim::v(if d.rotational { 45.0 } else { 20.0 })); }
                                    dim_field(ui, &mut d.offsets[i - 1], &vars, 60.0);
                                } else { ui.label("—"); }
                                ui.end_row();
                            }
                        });
                        if ui.button("Querschnitt einfügen").clicked() { act = Act::Sketch(Slot::Blend(d.sections.len())); }
                    }
                    ("Optionen", FeatureKind::Blend(d)) => {
                        ui.checkbox(&mut d.flip, "Verbundrichtung umkehren");
                        ui.label(RichText::new(if d.rotational { "Querschnitte werden um die Mittellinie von Querschnitt 1 gedreht und glatt verbunden." } else { "Querschnitte werden mit geraden (Regel-)Flächen verbunden." }).size(11.0).color(LABEL));
                    }
                    ("Platzierung", FeatureKind::Hole(d)) => {
                        labeled(ui, "Platzierung", |ui| { if collector(ui, &pn(self, &d.plane), pick == Some(PickTarget::Plane), 170.0).clicked() { act = Act::Pick(PickTarget::Plane); } });
                        ui.label(RichText::new("Typ: Linear — Versätze in der Platzierungsebene gemessen").size(11.0).color(LABEL));
                        egui::Grid::new("holeplace").show(ui, |ui| {
                            ui.label("Versatz X"); dim_field(ui, &mut d.x, &vars, 70.0); ui.end_row();
                            ui.label("Versatz Y"); dim_field(ui, &mut d.y, &vars, 70.0); ui.end_row();
                        });
                        ui.checkbox(&mut d.flip, "Bohrrichtung umkehren");
                    }
                    ("Form", FeatureKind::Hole(d)) => {
                        hole_shape_diagram(ui, d, &vars);
                    }
                    ("Sätze", FeatureKind::Round(d)) => {
                        let mut active = self.round_set.min(d.sets.len() - 1);
                        let mut delete = None;
                        egui::Grid::new("round_sets").striped(true).num_columns(4).show(ui, |ui| {
                            for (i, set) in d.sets.iter().enumerate() {
                                if ui.selectable_label(i == active, format!("Satz {}", i + 1)).clicked() { active = i; }
                                ui.label(format!("{} Kante(n)", set.edges.len()));
                                ui.label(if set.variable { format!("R {} → {}", set.radius.0, set.radius_end.0) } else { format!("R {}", set.radius.0) });
                                if d.sets.len() > 1 && ui.small_button("✕").clicked() { delete = Some(i); }
                                ui.end_row();
                            }
                        });
                        if ui.button("＋ Neuer Satz").clicked() { d.sets.push(RoundSet::new(d.sets[active].radius.value(&vars, 3.0))); active = d.sets.len() - 1; act = Act::Pick(PickTarget::Edges); }
                        if let Some(i) = delete { d.sets.remove(i); active = active.min(d.sets.len() - 1); }
                        ui.separator();
                        let set = &mut d.sets[active];
                        ui.label(RichText::new(format!("Referenzen von Satz {}", active + 1)).strong());
                        let mut remove = None;
                        for (i, e) in set.edges.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!("Kante {}   ({}, {}, {})", i + 1, fmt_num(e.m.x, 1), fmt_num(e.m.y, 1), fmt_num(e.m.z, 1)));
                                if ui.small_button("✕").clicked() { remove = Some(i); }
                            });
                        }
                        if let Some(i) = remove { set.edges.remove(i); }
                        labeled(ui, if set.variable { "Startradius" } else { "Radius" }, |ui| { dim_field(ui, &mut set.radius, &vars, 70.0); });
                        ui.checkbox(&mut set.variable, "Variabler Radius entlang der Kante");
                        if set.variable { labeled(ui, "Endradius", |ui| { dim_field(ui, &mut set.radius_end, &vars, 70.0); }); }
                        if ui.button(if pick == Some(PickTarget::Edges) { "Auswahl beendet" } else { "Kanten hinzufügen..." }).clicked() { act = Act::Pick(PickTarget::Edges); }
                        self.round_set = active;
                    }
                    ("Sätze", FeatureKind::Chamfer(ChamferDef { edges, .. })) => {
                        ui.label("Referenzen");
                        egui::Frame::NONE.stroke(Stroke::new(1.0_f32, BORDER)).inner_margin(4.0).show(ui, |ui| {
                            ui.set_min_height(70.0);
                            ui.set_width(290.0);
                            if edges.is_empty() { ui.label(RichText::new("Kanten im Grafikfenster wählen").color(LABEL)); }
                            let mut remove = None;
                            for (i, e) in edges.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(format!("Kante {}   ({}, {}, {})", i + 1, fmt_num(e.m.x, 1), fmt_num(e.m.y, 1), fmt_num(e.m.z, 1)));
                                    if ui.small_button("✕").clicked() { remove = Some(i); }
                                });
                            }
                            if let Some(i) = remove { edges.remove(i); }
                        });
                        if ui.button(if pick == Some(PickTarget::Edges) { "Auswahl beendet" } else { "Kanten hinzufügen..." }).clicked() { act = Act::Pick(PickTarget::Edges); }
                    }
                    ("Referenzen", FeatureKind::Shell(ShellDef { faces, .. })) | ("Referenzen", FeatureKind::Draft(DraftDef { faces, .. })) => {
                        ui.label("Flächen");
                        egui::Frame::NONE.stroke(Stroke::new(1.0_f32, BORDER)).inner_margin(4.0).show(ui, |ui| {
                            ui.set_min_height(60.0);
                            ui.set_width(290.0);
                            let mut remove = None;
                            for (i, fr) in faces.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(format!("Fläche {}   Normale ({}, {}, {})", i + 1, fmt_num(fr.normal.x, 2), fmt_num(fr.normal.y, 2), fmt_num(fr.normal.z, 2)));
                                    if ui.small_button("✕").clicked() { remove = Some(i); }
                                });
                            }
                            if let Some(i) = remove { faces.remove(i); }
                        });
                        if ui.button(if pick == Some(PickTarget::Faces) { "Auswahl beendet" } else { "Flächen hinzufügen..." }).clicked() { act = Act::Pick(PickTarget::Faces); }
                    }
                    ("Referenzen", FeatureKind::Rib(d)) => {
                        labeled(ui, "Skizzierebene", |ui| { if collector(ui, &pn(self, &d.plane), pick == Some(PickTarget::SketchPlane), 160.0).clicked() { act = Act::Pick(PickTarget::SketchPlane); } });
                        if ui.button(if d.sketch.is_empty() { "Skizze definieren..." } else { "Skizze editieren..." }).clicked() { act = Act::Sketch(Slot::Main); }
                        ui.label(RichText::new("Die offene Skizze wird bis zum Volumen geschlossen; die Rippe wächst auf der Seite, auf der das Volumen liegt.").size(11.0).color(LABEL));
                    }
                    ("Referenzen", FeatureKind::Helix(d)) => {
                        labeled(ui, "Profilebene", |ui| { if collector(ui, &pn(self, &d.plane), pick == Some(PickTarget::SketchPlane), 160.0).clicked() { act = Act::Pick(PickTarget::SketchPlane); } });
                        ui.horizontal(|ui| {
                            if ui.button("Profil editieren...").clicked() { act = Act::Sketch(Slot::Trajectory); }
                            if ui.button("Querschnitt editieren...").clicked() { act = Act::Sketch(Slot::Section); }
                        });
                        ui.label(RichText::new("Profil: Mittellinie als Achse und eine offene Kette, deren Abstand zur Achse den Radius vorgibt.").size(11.0).color(LABEL));
                    }
                    ("Optionen", FeatureKind::Helix(d)) => {
                        labeled(ui, "Steigung", |ui| { dim_field(ui, &mut d.pitch, &vars, 70.0); });
                        ui.checkbox(&mut d.left_handed, "Linksgängig (sonst rechtsgängig)");
                        ui.label(RichText::new("Querschnittsausrichtung: durch die Achse").size(11.0).color(LABEL));
                    }
                    ("Referenzen", FeatureKind::Thread(d)) => {
                        labeled(ui, "Gewindefläche", |ui| { if collector(ui, if d.face.is_some() { "Zylinderfläche" } else { "Fläche wählen" }, pick == Some(PickTarget::Surface), 160.0).clicked() { act = Act::Pick(PickTarget::Surface); } });
                        ui.checkbox(&mut d.flip, "Am anderen Ende der Fläche beginnen");
                        egui::Grid::new("thr").show(ui, |ui| {
                            ui.label("Durchmesser"); dim_field(ui, &mut d.diameter, &vars, 70.0); ui.end_row();
                            ui.label("Steigung"); dim_field(ui, &mut d.pitch, &vars, 70.0); ui.end_row();
                            ui.label("Länge"); dim_field(ui, &mut d.length, &vars, 70.0); ui.end_row();
                        });
                    }
                    ("Referenzen", FeatureKind::Mirror(d)) => {
                        labeled(ui, "Spiegelebene", |ui| { if collector(ui, &pn(self, &d.plane), pick == Some(PickTarget::Plane), 160.0).clicked() { act = Act::Pick(PickTarget::Plane); } });
                        ui.label("KEs (leer = gesamte Geometrie)");
                        if ui.button("KEs wählen...").clicked() { act = Act::Pick(PickTarget::Features); }
                    }
                    ("Referenzen", FeatureKind::Pattern(d)) => {
                        ui.label("Gemusterte KEs");
                        for id in &d.features { ui.label(format!("• {}", self.data.feature(*id).map(|f| f.name.clone()).unwrap_or_default())); }
                        if ui.button("KEs wählen...").clicked() { act = Act::Pick(PickTarget::Features); }
                    }
                    ("Optionen", FeatureKind::Pattern(_)) => {
                        ui.label(RichText::new("Musterelemente werden als identische Kopien der gewählten KEs regeneriert.").size(11.0).color(LABEL));
                    }
                    _ => {}
                }
                });
            });
        });
        if close { self.dash_panel = None; }
        if f != ed.feature { let nf = f; self.update_editor(|x| *x = nf); }
        self.run_act(act);
    }

    /// Creo's "Skizze" dialog opened from Platzierung ▸ Definieren... of a Profil or Drehen:
    /// sketch plane, view direction and the (standard) orientation reference.
    pub fn sketch_setup_dialog(&mut self, ctx: &egui::Context) {
        if self.sketch_dlg.is_none() || self.sketch.is_some() { return; }
        let Some(ed) = self.editor.clone() else { return };
        let pick = ed.pick;
        let plane = ed.feature.kind.sketch_plane();
        let shown = self.context.clone().unwrap_or_else(|| self.result.clone());
        let orient = plane.and_then(|p| shown.frame_for(&p)).and_then(|fr| default_orientation(&shown, &fr));
        let prev = self.previous_sketch_plane();
        let mut act = Act::None;
        let mut flip = false;
        let mut use_prev = false;
        let mut cancel = false;
        let mut x_open = true;
        egui::Window::new("Skizze").id(egui::Id::new("sketch_setup")).title_bar(false).collapsible(false).resizable(false).default_pos(pos2(self.viewport_size.0 + 20.0, 170.0)).show(ctx, |ui| {
            if super::creo::dialog_header(ui, "Skizze") { x_open = false; }
            ui.set_width(470.0);
            ui.add_space(6.0);
            ui.horizontal(|ui| { let _ = ui.selectable_label(true, RichText::new("Platzierung").size(13.5)); });
            egui::Frame::NONE.stroke(Stroke::new(1.0_f32, BORDER)).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
                ui.set_width(446.0);
                ui.spacing_mut().item_spacing.y = 8.0;
                section_rule(ui, "Skizzierebene");
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new("Ebene").size(13.5));
                    let active = pick == Some(PickTarget::SketchPlane);
                    let name = if plane.is_some() { self.plane_name(&plane) } else { String::new() };
                    if collector(ui, &name, active, 160.0).clicked() && !active { act = Act::Pick(PickTarget::SketchPlane); }
                    if ui.add_enabled(prev.is_some(), egui::Button::new(RichText::new("Vorherig. verwenden").size(13.5)).min_size(vec2(0.0, 26.0))).on_hover_text("Die Skizzierebene des vorherigen Skizzen-KEs verwenden").clicked() { use_prev = true; }
                });
                section_rule(ui, "Skizzenorientierung");
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new("Skizzen-Ansichtsrichtung").size(13.5));
                    if ui.add_enabled(plane.is_some(), egui::Button::new(RichText::new("Umkehren").size(13.5)).min_size(vec2(0.0, 26.0))).on_hover_text("Die Seite umkehren, von der auf die Skizzierebene geschaut wird (und in die das Profil zeigt)").clicked() { flip = true; }
                });
                egui::Grid::new("sk_orient").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                    ui.horizontal(|ui| { ui.add_space(12.0); ui.label(RichText::new("Referenz").size(13.5)); });
                    collector(ui, &orient.as_ref().map(|o| o.0.clone()).unwrap_or_default(), false, 300.0).on_hover_text("Standardreferenz: Freo richtet die Skizze nach der Standardbezugsebene aus");
                    ui.end_row();
                    ui.horizontal(|ui| { ui.add_space(12.0); ui.label(RichText::new("Orientierung").size(13.5)); });
                    ui.add_enabled_ui(false, |ui| {
                        egui::ComboBox::from_id_salt("sk_orient_dir").width(90.0).selected_text(orient.as_ref().map(|o| o.1).unwrap_or("")).show_ui(ui, |_| {});
                    });
                    ui.end_row();
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("Abbrechen").size(13.5)).min_size(vec2(110.0, 30.0))).clicked() { cancel = true; }
                    if ui.add_enabled(plane.is_some(), egui::Button::new(RichText::new("Skizze").size(13.5)).min_size(vec2(110.0, 30.0))).clicked() { act = Act::Sketch(Slot::Main); }
                });
            });
        });
        if use_prev {
            if let Some(p) = prev {
                self.update_editor(|f| f.kind.set_sketch_plane(p));
                if let Some(e) = &mut self.editor { e.pick = None; }
            }
        }
        if flip {
            self.update_editor(|f| match &mut f.kind { FeatureKind::Extrude(d) => d.flip = !d.flip, FeatureKind::Revolve(d) => d.flip = !d.flip, _ => {} });
        }
        if cancel || !x_open { self.cancel_sketch_dialog(); return; }
        self.run_act(act);
    }

    /// Setup ▸ Skizzen-Setup while sketching: the same "Skizze" dialog; Skizze moves the sketch to
    /// the chosen plane.
    pub fn sketch_resetup_dialog(&mut self, ctx: &egui::Context) {
        let Some((plane, flip)) = self.resetup else { return };
        let shown = self.context.clone().unwrap_or_else(|| self.result.clone());
        let orient = shown.frame_for(&plane).and_then(|fr| default_orientation(&shown, &fr));
        let prev = self.previous_sketch_plane();
        let (mut go, mut cancel, mut x_open, mut new_flip, mut new_plane) = (false, false, true, flip, plane);
        egui::Window::new("Skizze").id(egui::Id::new("sketch_resetup")).title_bar(false).collapsible(false).resizable(false).default_pos(pos2(self.viewport_size.0 + 20.0, 170.0)).show(ctx, |ui| {
            if super::creo::dialog_header(ui, "Skizze") { x_open = false; }
            ui.set_width(470.0);
            ui.add_space(6.0);
            ui.horizontal(|ui| { let _ = ui.selectable_label(true, RichText::new("Platzierung").size(13.5)); });
            egui::Frame::NONE.stroke(Stroke::new(1.0_f32, BORDER)).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
                ui.set_width(446.0);
                ui.spacing_mut().item_spacing.y = 8.0;
                section_rule(ui, "Skizzierebene");
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new("Ebene").size(13.5));
                    collector(ui, &self.plane_name(&Some(plane)), true, 160.0);
                    if ui.add_enabled(prev.is_some(), egui::Button::new(RichText::new("Vorherig. verwenden").size(13.5)).min_size(vec2(0.0, 26.0))).clicked() { if let Some(p) = prev { new_plane = p; } }
                });
                section_rule(ui, "Skizzenorientierung");
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new("Skizzen-Ansichtsrichtung").size(13.5));
                    if ui.add(egui::Button::new(RichText::new("Umkehren").size(13.5)).selected(flip).min_size(vec2(0.0, 26.0))).clicked() { new_flip = !flip; }
                });
                egui::Grid::new("sk_reorient").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                    ui.horizontal(|ui| { ui.add_space(12.0); ui.label(RichText::new("Referenz").size(13.5)); });
                    collector(ui, &orient.as_ref().map(|o| o.0.clone()).unwrap_or_default(), false, 300.0);
                    ui.end_row();
                    ui.horizontal(|ui| { ui.add_space(12.0); ui.label(RichText::new("Orientierung").size(13.5)); });
                    ui.add_enabled_ui(false, |ui| { egui::ComboBox::from_id_salt("sk_reorient_dir").width(90.0).selected_text(orient.as_ref().map(|o| o.1).unwrap_or("")).show_ui(ui, |_| {}); });
                    ui.end_row();
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("Abbrechen").size(13.5)).min_size(vec2(110.0, 30.0))).clicked() { cancel = true; }
                    if ui.add(egui::Button::new(RichText::new("Skizze").size(13.5)).min_size(vec2(110.0, 30.0))).clicked() { go = true; }
                });
            });
        });
        if cancel || !x_open { self.resetup = None; return; }
        if go {
            self.resetup = None;
            let same = self.editor.as_ref().and_then(|e| e.feature.kind.sketch_plane()) == Some(new_plane);
            if !same || new_flip { self.resetup_sketch(new_plane, new_flip); }
            return;
        }
        self.resetup = Some((new_plane, new_flip));
    }

    /// Creo-style dialog boxes for datum features and the sketch setup.
    pub fn datum_dialog(&mut self, ctx: &egui::Context) {
        let Some(ed) = self.editor.clone() else { return };
        if !Self::uses_dialog(&ed.feature.kind) || self.sketch.is_some() { return; }
        let vars = self.ed_vars();
        let mut f = ed.feature.clone();
        let mut act = Act::None;
        let pick = ed.pick;
        let title = match f.kind { FeatureKind::DatumPlane(_) => "Bezugsebene", FeatureKind::DatumAxis(_) => "Bezugsachse", FeatureKind::DatumPoint(_) => "Bezugspunkt", FeatureKind::DatumCsys(_) => "Koordinatensystem", FeatureKind::Curve(_) => "Kurve durch Punkte", _ => "Skizze" };
        let tabs: &[&str] = match f.kind {
            FeatureKind::Sketch(_) | FeatureKind::Curve(_) => &["Platzierung", "Eigenschaften"],
            FeatureKind::DatumCsys(_) => &["Ursprung", "Ausrichtung", "Eigenschaften"],
            _ => &["Platzierung", "Anzeige", "Eigenschaften"],
        };
        let err = self.editor_error();
        let mut x_open = true;
        egui::Window::new(title).id(egui::Id::new("datum_dialog")).title_bar(false).collapsible(false).resizable(false).default_pos(pos2(320.0, 170.0)).show(ctx, |ui| {
            if super::creo::dialog_header(ui, title) { x_open = false; }
            ui.set_width(330.0);
            ui.horizontal(|ui| {
                for (i, t) in tabs.iter().enumerate() { if ui.selectable_label(self.dialog_tab == i as u8, *t).clicked() { self.dialog_tab = i as u8; } }
            });
            ui.separator();
            let tab = tabs.get(self.dialog_tab as usize).copied().unwrap_or(tabs[0]);
            let pn = |s: &App, r: &Option<PlaneRef>| s.plane_name(r);
            match (tab, &mut f.kind) {
                ("Eigenschaften", _) => { labeled(ui, "Name", |ui| { ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(200.0)); }); }
                ("Platzierung", FeatureKind::DatumPlane(d)) => {
                    ui.label("Referenzen");
                    ui.horizontal(|ui| {
                        if collector(ui, &pn(self, &d.reference), pick == Some(PickTarget::Plane), 170.0).clicked() { act = Act::Pick(PickTarget::Plane); }
                        egui::ComboBox::from_id_salt("dpk").width(110.0).selected_text(if d.kind == PlaneKind::Angle { "Durch Achse" } else { "Versatz" }).show_ui(ui, |ui| {
                            ui.selectable_value(&mut d.kind, PlaneKind::Offset, "Versatz");
                            ui.selectable_value(&mut d.kind, PlaneKind::Angle, "Durch Achse");
                        });
                    });
                    if d.kind == PlaneKind::Angle {
                        ui.horizontal(|ui| {
                            let an = d.axis.map(|a| self.axis_name(&a)).unwrap_or("Eine Achse wählen".into());
                            if collector(ui, &an, pick == Some(PickTarget::Plane2), 170.0).clicked() { act = Act::Pick(PickTarget::Plane2); }
                            ui.label("Durch");
                        });
                        ui.group(|ui| { labeled(ui, "Drehung", |ui| { dim_field(ui, &mut d.angle, &vars, 80.0); ui.label("°"); }); });
                    } else {
                        ui.group(|ui| { labeled(ui, "Versatz   Verschiebung", |ui| { dim_field(ui, &mut d.offset, &vars, 80.0); }); });
                    }
                }
                ("Anzeige", FeatureKind::DatumPlane(d)) => { if ui.button("Umkehren").clicked() { d.flip = !d.flip; } ui.label(RichText::new("Kehrt die Ebene auf die andere Seite ihrer Referenz.").size(11.0).color(LABEL)); }
                ("Platzierung", FeatureKind::DatumAxis(d)) => {
                    egui::ComboBox::from_id_salt("dak").width(160.0).selected_text(match d.kind { AxisKind::TwoPlanes => "Schnitt zweier Ebenen", AxisKind::World => "Koordinatensystemachse", AxisKind::Normal => "Senkrecht zur Ebene" }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut d.kind, AxisKind::TwoPlanes, "Schnitt zweier Ebenen");
                        ui.selectable_value(&mut d.kind, AxisKind::World, "Koordinatensystemachse");
                        ui.selectable_value(&mut d.kind, AxisKind::Normal, "Senkrecht zur Ebene");
                    });
                    ui.label("Referenzen");
                    match d.kind {
                        AxisKind::TwoPlanes => {
                            ui.horizontal(|ui| { if collector(ui, &pn(self, &d.p1), pick == Some(PickTarget::Plane), 200.0).clicked() { act = Act::Pick(PickTarget::Plane); } ui.label("Durch"); });
                            ui.horizontal(|ui| { if collector(ui, &pn(self, &d.p2), pick == Some(PickTarget::Plane2), 200.0).clicked() { act = Act::Pick(PickTarget::Plane2); } ui.label("Durch"); });
                        }
                        AxisKind::World => { ui.horizontal(|ui| { for (i, n) in ["X", "Y", "Z"].iter().enumerate() { ui.selectable_value(&mut d.world, i as u8, format!("BASIS: {}", n)); } }); }
                        AxisKind::Normal => {
                            ui.horizontal(|ui| { if collector(ui, &pn(self, &d.p1), pick == Some(PickTarget::Plane2), 200.0).clicked() { act = Act::Pick(PickTarget::Plane2); } ui.label("Normal"); });
                            ui.horizontal(|ui| { labeled(ui, "Versatz X", |ui| { dim_field(ui, &mut d.x, &vars, 60.0); }); labeled(ui, "Y", |ui| { dim_field(ui, &mut d.y, &vars, 60.0); }); });
                        }
                    }
                }
                ("Platzierung", FeatureKind::DatumPoint(d)) => {
                    ui.label("Versatz von BASIS (kartesisch)");
                    egui::Grid::new("dpt").show(ui, |ui| {
                        ui.label("X"); dim_field(ui, &mut d.x, &vars, 80.0); ui.end_row();
                        ui.label("Y"); dim_field(ui, &mut d.y, &vars, 80.0); ui.end_row();
                        ui.label("Z"); dim_field(ui, &mut d.z, &vars, 80.0); ui.end_row();
                    });
                }
                ("Platzierung", FeatureKind::Sketch(d)) => {
                    ui.label(RichText::new("Skizzierebene").strong());
                    labeled(ui, "Ebene", |ui| { if collector(ui, &pn(self, &d.plane), pick == Some(PickTarget::SketchPlane), 200.0).clicked() { act = Act::Pick(PickTarget::SketchPlane); } });
                    ui.label(RichText::new("Skizzenausrichtung").strong());
                    ui.label(RichText::new("Referenz: Standard (aus der Ebene abgeleitet)    Ausrichtung: Rechts").size(11.0).color(LABEL));
                }
                ("Ursprung", FeatureKind::DatumCsys(d)) => {
                    ui.label("Referenz: BASIS   •   Versatztyp: Kartesisch");
                    egui::Grid::new("csysorg").show(ui, |ui| {
                        ui.label("X"); dim_field(ui, &mut d.x, &vars, 80.0); ui.end_row();
                        ui.label("Y"); dim_field(ui, &mut d.y, &vars, 80.0); ui.end_row();
                        ui.label("Z"); dim_field(ui, &mut d.z, &vars, 80.0); ui.end_row();
                    });
                }
                ("Ausrichtung", FeatureKind::DatumCsys(d)) => {
                    ui.label("Bezüglich ausgewähltem Koordinatensystem ausrichten (Drehung um X, dann Y, dann Z)");
                    egui::Grid::new("csysrot").show(ui, |ui| {
                        ui.label("Um X"); dim_field(ui, &mut d.rx, &vars, 80.0); ui.label("°"); ui.end_row();
                        ui.label("Um Y"); dim_field(ui, &mut d.ry, &vars, 80.0); ui.label("°"); ui.end_row();
                        ui.label("Um Z"); dim_field(ui, &mut d.rz, &vars, 80.0); ui.label("°"); ui.end_row();
                    });
                }
                ("Platzierung", FeatureKind::Curve(d)) => {
                    ui.label("Punkte");
                    egui::Frame::NONE.stroke(Stroke::new(1.0_f32, BORDER)).inner_margin(4.0).show(ui, |ui| {
                        ui.set_min_height(70.0);
                        ui.set_width(310.0);
                        if d.points.is_empty() { ui.label(RichText::new("Bezugspunkte der Reihe nach wählen").color(LABEL)); }
                        let mut remove = None;
                        for (i, id) in d.points.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!("{}   {}", i + 1, self.data.feature(*id).map(|f| f.name.clone()).unwrap_or_default()));
                                if ui.small_button("✕").clicked() { remove = Some(i); }
                            });
                        }
                        if let Some(i) = remove { d.points.remove(i); }
                    });
                    if ui.button(if pick == Some(PickTarget::Points) { "Auswahl beendet" } else { "Punkte hinzufügen..." }).clicked() { act = Act::Pick(PickTarget::Points); }
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut d.spline, true, "Spline");
                        ui.selectable_value(&mut d.spline, false, "Gerade Segmente");
                    });
                }
                ("Anzeige", _) => { ui.label(RichText::new("Keine Anzeigeoptionen für diesen Bezug.").color(LABEL)); }
                _ => {}
            }
            if let Some(e) = &err { ui.label(RichText::new(format!("⚠ {}", e)).color(Color32::from_rgb(190, 40, 30))); }
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_sized([72.0, 24.0], egui::Button::new("Abbrechen")).clicked() { act = Act::Cancel; }
                    let is_sketch = matches!(f.kind, FeatureKind::Sketch(_));
                    let ok_label = if is_sketch { "Skizze" } else { "OK" };
                    if ui.add_enabled(err.is_none() || is_sketch, egui::Button::new(ok_label).min_size(vec2(72.0, 24.0))).clicked() {
                        act = if is_sketch { Act::Sketch(Slot::Main) } else { Act::Ok };
                    }
                });
            });
        });
        if f != ed.feature { let nf = f; self.update_editor(|x| *x = nf); }
        if !x_open { act = Act::Cancel; }
        self.run_act(act);
    }
}

/// Small annotated diagram of the hole profile (Creo's Shape panel).
fn hole_shape_diagram(ui: &mut egui::Ui, d: &mut HoleDef, vars: &Vars) {
    let (rect, _) = ui.allocate_exact_size(vec2(300.0, 170.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 2.0, Color32::from_rgb(250, 251, 252));
    let r = d.diameter.value(vars, 10.0) / 2.0;
    let depth = if d.through_all { r * 5.0 } else { d.depth.value(vars, 20.0) };
    let prof = crate::model::regen::hole_profile(d, vars, depth).unwrap_or_default();
    if prof.is_empty() { return; }
    let maxr = prof.iter().map(|q| q.x).fold(0.0, f64::max).max(1e-6);
    let maxd = prof.iter().map(|q| q.y).fold(0.0, f64::max).max(1e-6);
    let s = ((rect.width() as f64 * 0.32) / maxr).min((rect.height() as f64 - 30.0) / maxd);
    let cx = rect.center().x;
    let top = rect.top() + 16.0;
    let to = |x: f64, y: f64| pos2(cx + (x * s) as f32, top + (y.max(0.0) * s) as f32);
    for sign in [-1.0, 1.0] {
        let pts: Vec<egui::Pos2> = prof.iter().map(|q| to(q.x * sign, q.y)).collect();
        p.add(egui::Shape::line(pts, Stroke::new(1.6_f32, Color32::from_rgb(30, 60, 140))));
    }
    p.extend(egui::Shape::dashed_line(&[pos2(cx, top - 8.0), pos2(cx, rect.bottom() - 4.0)], Stroke::new(1.0_f32, Color32::GRAY), 6.0, 3.0));
    let r = crate::model::regen::hole_drill(d, vars).unwrap_or(r * 2.0) / 2.0;
    if d.ty == HoleType::Standard && d.tapped {
        if let Some(spec) = crate::model::threads::find(d.standard, &d.size) {
            let td = if d.through_all { depth } else { d.thread_depth.value(vars, depth).min(depth) };
            for sign in [-1.0, 1.0] {
                p.extend(egui::Shape::dashed_line(&[to(sign * spec.major / 2.0, 0.0), to(sign * spec.major / 2.0, td)], Stroke::new(1.2_f32, Color32::from_rgb(220, 120, 30)), 4.0, 2.0));
            }
            p.text(pos2(cx, top - 10.0), Align2::CENTER_CENTER, crate::model::threads::callout(d.standard, spec), FontId::proportional(11.5), Color32::from_rgb(200, 100, 20));
        }
    }
    p.text(to(r, depth * 0.5) + vec2(8.0, 0.0), Align2::LEFT_CENTER, format!("⌀{}", fmt_num(r * 2.0, 2)), FontId::proportional(11.5), STRONG_DIM);
    if !d.through_all { p.text(to(-r, depth * 0.5) - vec2(8.0, 0.0), Align2::RIGHT_CENTER, fmt_num(depth, 2), FontId::proportional(11.5), STRONG_DIM); }
    else { p.text(pos2(cx, rect.bottom() - 10.0), Align2::CENTER_CENTER, "Durch alle", FontId::proportional(11.0), STRONG_DIM); }
}

/// Description shown at the right end of a dashboard (Creo 10).
fn feature_help(k: &FeatureKind) -> Option<&'static str> {
    match k {
        FeatureKind::Extrude(_) => Some("3D-Geometrie durch Verschieben eines Schnitts senkrecht zur Skizzierebene als Volumenkörper oder Fläche erzeugen."),
        _ => None,
    }
}

const STRONG_DIM: Color32 = Color32::from_rgb(24, 46, 150);
