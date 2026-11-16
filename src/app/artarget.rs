//! Werkzeuge ▸ Erweiterte Realität ▸ Ziel: the "Räumliches Ziel" and "ThingMark" ribbon tabs.
//! A target sits on one side of the model's bounding box, at a distance; it is saved with the
//! part and drawn in the graphics window.

use super::dlgkit::{combo, field, lbl, LINE};
use super::icons::{self, Icon};
use super::ribbon::{group, large, theme::*};
use super::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind};

pub const SIDES: [&str; 6] = ["Unten", "Oben", "Vorne", "Hinten", "Links", "Rechts"];
const CYAN: Color32 = Color32::from_rgb(40, 190, 230);

pub struct ArDlg {
    pub thing: bool,
    /// 0 Standardebene, 1 Benutzerdefinierte Ebene, 2 Erweitert
    pub kind: usize,
    pub side: usize,
    pub offset: String,
    pub width: String,
    pub handle: bool,
    pub paused: bool,
    /// 0 none, 1 Platzierung, 2 Eigenschaften
    pub sub: usize,
    pub name: String,
}

fn num(s: &str, d: f64) -> f64 {
    s.trim().replace(',', ".").parse::<f64>().unwrap_or(d)
}

impl App {
    /// Opens the Räumliches Ziel tab (or ThingMark, as chosen in the Ziel ▾ menu).
    pub fn start_ar_target(&mut self) {
        if self.editor.is_some() || self.sketch.is_some() || self.in_asm() {
            self.say("Zuerst das aktive Werkzeug beenden.", MsgKind::Warning);
            return;
        }
        let thing = self.creo.tools.thingmark;
        let n = self.data.ar_targets.iter().filter(|t| t.thing == thing).count() + 1;
        let b = self.result.topo.bbox;
        let w = if b.is_empty() { 50.0 } else { (b.size().x.min(b.size().z) * 0.6).max(1.0) };
        self.creo.tools.ar = Some(ArDlg {
            thing,
            kind: 0,
            side: 0,
            offset: "0.00".into(),
            width: format!("{:.3}", w),
            handle: true,
            paused: false,
            sub: 1,
            name: format!("{}_{}", if thing { "THINGMARK" } else { "SPATIALTARGET" }, n),
        });
        self.tab = Tab::Context;
        self.say(
            if thing {
                "Ebene, Fläche oder Sammelfläche auswählen, um das ThingMark hinzuzufügen."
            } else {
                "Ebene, Fläche oder Sammelfläche auswählen, um die Position des augmentierten CAD-Objekts festzulegen."
            },
            MsgKind::Prompt,
        );
    }

    pub fn ar_tab_name(&self) -> Option<String> {
        self.creo.tools.ar.as_ref().map(|a| if a.thing { "ThingMark".to_string() } else { "Räumliches Ziel".to_string() })
    }

    /// The dashboard of the tab.
    pub fn ar_ribbon(&mut self, ui: &mut egui::Ui) {
        let Some(mut a) = self.creo.tools.ar.take() else { return };
        let (mut ok, mut cancel) = (false, false);
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.add_space(26.0);
            let (r, _) = ui.allocate_exact_size(vec2(60.0, 44.0), Sense::hover());
            icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(40.0, 40.0)), if a.thing { Icon::TlThingMark } else { Icon::TlTarget }, true);
        });
        group(ui, "Platzierungstyp", |ui| {
            for (k, (ic, n)) in [(Icon::Plane, "Standardebene"), (Icon::DatumDisplay, "Benutzerdefinierte\nEbene"), (Icon::Layers, "Erweitert")].into_iter().enumerate() {
                if large(ui, ic, n, true, a.kind == k).clicked() {
                    a.kind = k;
                }
            }
        });
        group(ui, "Einstellungen", |ui| {
            ui.vertical(|ui| {
                ui.add_space(22.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.add_space(6.0);
                    lbl(ui, "Ebene:");
                    combo(ui, "ar_side", 110.0, &mut a.side, &SIDES, a.kind == 0, Some(Icon::Plane));
                    if a.thing {
                        ui.add_space(8.0);
                        lbl(ui, "Breite:");
                        field(ui, &mut a.width, 96.0, true);
                    }
                    ui.add_space(8.0);
                    lbl(ui, "Abstand");
                    field(ui, &mut a.offset, 96.0, true);
                    ui.add_space(6.0);
                });
            });
        });
        group(ui, "Verschieben", |ui| {
            if large(ui, Icon::DragComp, "Ziehgriff\nanzeigen", true, a.handle).clicked() {
                a.handle = !a.handle;
            }
        });
        ui.add_space(14.0);
        ui.vertical(|ui| {
            ui.add_space(20.0);
            let (r, resp) = ui.allocate_exact_size(vec2(46.0, 46.0), Sense::click());
            if a.paused {
                ui.painter().rect_filled(r, 2.0, SEL_BG);
            } else if resp.hovered() {
                ui.painter().rect_filled(r, 2.0, HOVER_BG);
            }
            for dx in [-7.0f32, 7.0] {
                ui.painter().rect_filled(Rect::from_center_size(r.center() + vec2(dx, 0.0), vec2(8.0, 26.0)), 0.0, Color32::from_rgb(40, 44, 50));
            }
            if resp.on_hover_text("Werkzeug anhalten, um ein asynchrones Werkzeug zu verwenden").clicked() {
                a.paused = !a.paused;
            }
        });
        ui.add_space(14.0);
        group(ui, "", |ui| {
            if large(ui, Icon::Ok, "OK", true, true).clicked() {
                ok = true;
            }
            if large(ui, Icon::Cancel, "Abbrechen", true, false).clicked() {
                cancel = true;
            }
        });
        ui.add_space(10.0);
        // Creo's help box on the right of the dashboard
        ui.vertical(|ui| {
            ui.add_space(4.0);
            egui::Frame::NONE.fill(Color32::from_rgb(226, 230, 232)).inner_margin(8.0).show(ui, |ui| {
                ui.set_width(300.0);
                ui.set_min_height(82.0);
                ui.label(
                    egui::RichText::new(if a.thing {
                        "Einer Ebene, Fläche oder Sammelfläche ein ThingMark hinzufügen, den zu scannenden Code für die erweiterte Realität."
                    } else {
                        "Ebene, Fläche oder Sammelfläche auswählen, um die Position des augmentierten CAD-Objekts beim Veröffentlichen festzulegen."
                    })
                    .size(13.0)
                    .color(TEXT),
                );
                if ui.link(egui::RichText::new("Mehr lesen...").size(13.0)).clicked() {
                    self.windows.controls = true;
                }
            });
        });
        if ok {
            let t = ArTarget { thing: a.thing, side: a.side as u8, offset: num(&a.offset, 0.0), width: num(&a.width, 50.0).abs().max(0.1), name: a.name.clone() };
            let label = format!("{} Regenerierung erfolgreich beendet.", if a.thing { "THINGMARK" } else { "SPATIALTARGET" });
            self.commit(&label, |d| d.ar_targets.push(t));
            self.tab = Tab::Tools;
        } else if cancel {
            self.tab = Tab::Tools;
        } else {
            self.creo.tools.ar = Some(a);
        }
    }

    /// The Platzierung / Eigenschaften tabs under the dashboard with their slide-down panel.
    pub fn ar_panel(&mut self, ctx: &egui::Context) {
        let Some(a) = self.creo.tools.ar.as_mut() else { return };
        if self.tab != Tab::Context {
            return;
        }
        let top = ctx.available_rect().top();
        egui::Area::new(egui::Id::new("ar_panel")).order(egui::Order::Foreground).fixed_pos(pos2(470.0, top)).show(ctx, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            ui.horizontal(|ui| {
                for (k, n) in ["Platzierung", "Eigenschaften"].into_iter().enumerate() {
                    let (r, resp) = ui.allocate_exact_size(vec2(132.0, 28.0), Sense::click());
                    let on = a.sub == k + 1;
                    ui.painter().rect_filled(r, 0.0, if on { Color32::from_rgb(250, 250, 250) } else { Color32::from_rgb(230, 233, 236) });
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, n, FontId::proportional(14.0), TEXT);
                    if resp.clicked() {
                        a.sub = if on { 0 } else { k + 1 };
                    }
                }
            });
            if a.sub == 0 {
                return;
            }
            egui::Frame::NONE.fill(Color32::from_rgb(250, 250, 250)).stroke(Stroke::new(1.0_f32, LINE)).inner_margin(12.0).show(ui, |ui| {
                ui.set_width(180.0);
                ui.spacing_mut().item_spacing = vec2(6.0, 8.0);
                ui.visuals_mut().override_text_color = Some(TEXT);
                if a.sub == 1 {
                    combo(ui, "ar_side2", 170.0, &mut a.side, &SIDES, a.kind == 0, Some(Icon::Plane));
                    ui.horizontal(|ui| {
                        lbl(ui, "Abstand");
                        field(ui, &mut a.offset, 110.0, true);
                    });
                } else {
                    ui.horizontal(|ui| {
                        lbl(ui, "Name");
                        field(ui, &mut a.name, 130.0, true);
                    });
                }
            });
        });
    }

    /// Corners of a target on `side` of the model's bounding box, `offset` away from it.
    fn ar_corners(&self, side: u8, offset: f64, width: Option<f64>) -> [V3; 4] {
        let b = self.displayed().topo.bbox;
        let (c, h) = if b.is_empty() { (V3::ZERO, V3::splat(50.0)) } else { (b.center(), b.size() * 0.5) };
        let (n, u, v) = match side {
            0 => (V3::NEG_Y, V3::X, V3::Z),
            1 => (V3::Y, V3::X, V3::Z),
            2 => (V3::Z, V3::X, V3::Y),
            3 => (V3::NEG_Z, V3::X, V3::Y),
            4 => (V3::NEG_X, V3::Z, V3::Y),
            _ => (V3::X, V3::Z, V3::Y),
        };
        let o = c + n * (h.dot(n.abs()) + offset);
        let (hu, hv) = match width {
            Some(w) => (w * 0.5, w * 0.5),
            None => (h.dot(u.abs()) * 1.15, h.dot(v.abs()) * 1.15),
        };
        [o - u * hu - v * hv, o + u * hu - v * hv, o + u * hu + v * hv, o - u * hu + v * hv]
    }

    /// Saved targets and the one being placed, drawn over the model.
    pub fn draw_ar_targets(&self, painter: &egui::Painter, to_screen: &dyn Fn(V3) -> Option<Pos2>) {
        let mut list: Vec<(bool, u8, f64, f64, bool)> = self.data.ar_targets.iter().map(|t| (t.thing, t.side, t.offset, t.width, false)).collect();
        if let Some(a) = &self.creo.tools.ar {
            list.push((a.thing, a.side as u8, num(&a.offset, 0.0), num(&a.width, 50.0).abs().max(0.1), true));
        }
        for (thing, side, offset, width, live) in list {
            let cs = self.ar_corners(side, offset, if thing { Some(width) } else { None });
            let pts: Vec<Pos2> = cs.iter().filter_map(|p| to_screen(*p)).collect();
            if pts.len() < 4 {
                continue;
            }
            painter.add(egui::Shape::convex_polygon(pts.clone(), Color32::from_rgba_unmultiplied(40, 190, 230, if live { 70 } else { 36 }), Stroke::NONE));
            painter.add(egui::Shape::closed_line(pts.clone(), Stroke::new(if live { 2.4_f32 } else { 1.6 }, CYAN)));
            let mid = |a: V3, b: V3, t: f64| a + (b - a) * t;
            let at = |s: f64, t: f64| to_screen(mid(mid(cs[0], cs[1], s), mid(cs[3], cs[2], s), t));
            if thing {
                // the code of a ThingMark: a hexagon with its inner mark
                for (r, w) in [(0.42, 2.0f32), (0.24, 1.4)] {
                    let hex: Vec<Pos2> = (0..6)
                        .filter_map(|i| {
                            let ang = (30.0 + 60.0 * i as f64).to_radians();
                            at(0.5 + r * ang.cos(), 0.5 + r * ang.sin())
                        })
                        .collect();
                    if hex.len() == 6 {
                        painter.add(egui::Shape::closed_line(hex, Stroke::new(w, Color32::from_rgb(40, 44, 50))));
                    }
                }
            } else if let (Some(a), Some(b), Some(c), Some(d)) = (at(0.5, 0.42), at(0.5, 0.58), at(0.42, 0.5), at(0.58, 0.5)) {
                painter.line_segment([a, b], Stroke::new(1.6_f32, CYAN));
                painter.line_segment([c, d], Stroke::new(1.6_f32, CYAN));
            }
            if live && self.creo.tools.ar.as_ref().is_some_and(|a| a.handle) {
                if let Some(c) = at(0.5, 0.5) {
                    painter.circle(c, 6.0, Color32::from_rgb(250, 190, 30), Stroke::new(1.2_f32, Color32::from_rgb(120, 80, 0)));
                    let r = Rect::from_min_size(c + vec2(60.0, -11.0), vec2(56.0, 22.0));
                    painter.line_segment([c, r.left_center()], Stroke::new(1.0_f32, Color32::from_rgb(240, 230, 40)));
                    painter.rect_filled(r, 0.0, Color32::from_rgb(24, 28, 34));
                    painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, Color32::from_rgb(240, 230, 40)), StrokeKind::Inside);
                    painter.text(r.center(), Align2::CENTER_CENTER, format!("{:.3}", offset), FontId::proportional(13.0), Color32::from_rgb(250, 240, 60));
                }
            }
        }
    }

    /// Publizieren / Teilen: writes the experience description (model and targets) to a file.
    pub fn publish_ar(&mut self, share: bool) {
        if self.data.ar_targets.is_empty() {
            self.say("Zuerst mit „Ziel“ ein räumliches Ziel oder ein ThingMark festlegen.", MsgKind::Warning);
            return;
        }
        let mut text = format!("ERWEITERTE-REALITÄT-ERFAHRUNG\nModell: {}\n\n", self.data.name.to_uppercase());
        for t in &self.data.ar_targets {
            text += &format!(
                "{:<20}{:<18}Ebene {:<8}Abstand {:.3}{}\n",
                t.name,
                if t.thing { "ThingMark" } else { "Räumliches Ziel" },
                SIDES[(t.side as usize).min(5)],
                t.offset,
                if t.thing { format!("   Breite {:.3}", t.width) } else { String::new() }
            );
        }
        let name = format!("{}_ar.txt", self.data.name.to_lowercase());
        if share {
            self.creo.tools.info = Some((format!("Informationsfenster ({})", name), text));
        } else {
            self.save_text("Erweiterte Realität publizieren", &name, &text);
        }
    }
}
