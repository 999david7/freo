//! Kombinations-Zustände (Anmerkungen erstellen): Neu / Aktualisieren in the ribbon and Creo's
//! state tabs along the bottom of the graphics window (◀◀ ◀ ▶ ▶▶, +, Comb0001 …, Standard Alle).

use super::render::Style;
use super::ribbon::theme::*;
use super::*;
use crate::model::features::ComboState;
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};

const BAR_H: f32 = 40.0;

impl App {
    fn capture_combo(&self, name: String) -> ComboState {
        ComboState {
            name,
            rot: self.camera.rot,
            target: self.camera.target,
            scale: self.camera.scale,
            style: Style::ALL.iter().position(|s| *s == self.style).unwrap_or(0) as u8,
            transparent: self.transparent,
        }
    }

    /// Neu: a new state from the current view, named Comb0001, Comb0002 …, and made active.
    pub fn combo_new(&mut self) {
        let next = self.data.combos.iter().filter_map(|c| c.name.strip_prefix("Comb").and_then(|n| n.parse::<u32>().ok())).max().unwrap_or(0) + 1;
        let state = self.capture_combo(format!("Comb{:04}", next));
        let name = state.name.clone();
        self.commit("", |d| d.combos.push(state));
        self.creo.combo_active = Some(self.data.combos.len() - 1);
        self.say(format!("Kombinations-Zustand {} erzeugt und aktiviert.", name), MsgKind::Success);
    }

    /// Aktualisieren: the active state takes the current view and display.
    pub fn combo_update(&mut self) {
        let Some(i) = self.creo.combo_active.filter(|i| *i < self.data.combos.len()) else {
            let state = self.capture_combo("Standard Alle".into());
            self.commit("Kombinations-Zustand Standard Alle aktualisiert.", |d| d.combo_default = Some(state));
            return;
        };
        let state = self.capture_combo(self.data.combos[i].name.clone());
        let name = state.name.clone();
        self.commit("", |d| d.combos[i] = state);
        self.say(format!("Kombinations-Zustand {} aktualisiert.", name), MsgKind::Success);
    }

    /// Activates a state (None = Standard Alle: standard orientation, fitted).
    pub fn combo_activate(&mut self, i: Option<usize>) {
        self.push_view();
        self.creo.combo_active = i;
        let state = match i { Some(i) => self.data.combos.get(i).cloned(), None => self.data.combo_default.clone() };
        match state {
            Some(c) => {
                self.camera.rot = c.rot;
                self.camera.target = c.target;
                self.camera.scale = c.scale;
                if let Some(s) = Style::ALL.get(c.style as usize) { self.style = *s; }
                self.transparent = c.transparent;
            }
            None => {
                self.camera.rot = camera::StdView::Standard.rotation();
                self.fit_pending = true;
            }
        }
    }

    /// The state tabs along the bottom of the graphics window (shown on Anmerkungen erstellen).
    pub fn combo_bar(&mut self, ctx: &egui::Context, rect: Rect) {
        if self.tab != Tab::Annot3d || self.sketch.is_some() || self.in_asm() { return; }
        let n = self.data.combos.len();
        let active = self.creo.combo_active.filter(|i| *i < n);
        // order of the tabs: the states, then Standard Alle
        let order: Vec<Option<usize>> = (0..n).map(Some).chain([None]).collect();
        let pos = order.iter().position(|o| *o == active).unwrap_or(n);
        let mut go: Option<Option<usize>> = None;
        let mut new = false;
        let bar = Rect::from_min_size(pos2(rect.left(), rect.bottom() - BAR_H), vec2(rect.width(), BAR_H));
        egui::Area::new(egui::Id::new("combo_bar")).fixed_pos(bar.min).order(egui::Order::Middle).show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(bar.size(), Sense::hover());
            let p = ui.painter();
            p.rect_filled(r, 0.0, Color32::from_rgb(236, 238, 240));
            p.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0_f32, Color32::from_rgb(196, 200, 206)));
            let mut x = r.left() + 6.0;
            let small = |ui: &mut egui::Ui, x: &mut f32, glyph: &str, enabled: bool, tip: &str| -> bool {
                let b = Rect::from_min_size(pos2(*x, r.top() + 7.0), vec2(28.0, 26.0));
                *x += 30.0;
                let resp = ui.interact(b, ui.id().with(("combo_nav", glyph)), if enabled { Sense::click() } else { Sense::hover() }).on_hover_text(tip);
                if enabled && resp.hovered() { ui.painter().rect_filled(b, 2.0, HOVER_BG); }
                ui.painter().text(b.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(13.0), if enabled { Color32::from_rgb(70, 74, 80) } else { Color32::from_rgb(180, 184, 188) });
                resp.clicked()
            };
            if small(ui, &mut x, "⏮", pos > 0, "Erster Zustand") { go = Some(order[0]); }
            if small(ui, &mut x, "◀", pos > 0, "Vorheriger Zustand") { go = Some(order[pos - 1]); }
            if small(ui, &mut x, "▶", pos + 1 < order.len(), "Nächster Zustand") { go = Some(order[pos + 1]); }
            if small(ui, &mut x, "⏭", pos + 1 < order.len(), "Letzter Zustand") { go = Some(order[order.len() - 1]); }
            x += 8.0;
            // + and the tabs sit on one framed strip
            let plus = Rect::from_min_size(pos2(x, r.top() + 4.0), vec2(34.0, BAR_H - 6.0));
            let resp = ui.interact(plus, ui.id().with("combo_plus"), Sense::click()).on_hover_text("Neuen Kombinations-Zustand erzeugen und aktivieren.");
            let p = ui.painter();
            p.rect_filled(plus, 0.0, if resp.hovered() { HOVER_BG } else { Color32::from_rgb(244, 245, 246) });
            p.rect_stroke(plus, 0.0, Stroke::new(1.0_f32, Color32::from_rgb(196, 200, 206)), egui::StrokeKind::Inside);
            p.text(plus.center(), Align2::CENTER_CENTER, "+", FontId::proportional(17.0), TEXT);
            if resp.clicked() { new = true; }
            x = plus.right();
            for o in &order {
                let label = match o { Some(i) => self.data.combos[*i].name.clone(), None => "Standard Alle *".to_string() };
                let font = FontId::proportional(15.0);
                let w = ui.fonts(|f| f.layout_no_wrap(label.clone(), font.clone(), TEXT).size().x) + 26.0;
                let t = Rect::from_min_size(pos2(x, r.top() + 4.0), vec2(w, BAR_H - 6.0));
                if t.left() > r.right() { break; }
                x += w;
                let sel = *o == active;
                let resp = ui.interact(t, ui.id().with(("combo_tab", *o)), Sense::click());
                let p = ui.painter();
                p.rect_filled(t, 0.0, if sel { Color32::WHITE } else if resp.hovered() { HOVER_BG } else { Color32::from_rgb(244, 245, 246) });
                p.rect_stroke(t, 0.0, Stroke::new(1.0_f32, Color32::from_rgb(196, 200, 206)), egui::StrokeKind::Inside);
                p.text(t.center(), Align2::CENTER_CENTER, &label, font.clone(), TEXT);
                // the active state's name in bold
                if sel { p.text(t.center() + vec2(0.6, 0.0), Align2::CENTER_CENTER, &label, font, TEXT); }
                if resp.clicked() && !sel { go = Some(*o); }
            }
        });
        if new { self.combo_new(); }
        if let Some(o) = go { self.combo_activate(o); }
    }
}
