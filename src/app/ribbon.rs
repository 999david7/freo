//! Creo-style window chrome: title bar with Quick Access Toolbar, tab strip with the green
//! File tab, ribbon widgets and the Model / Analysis / View / Tools / Sketch ribbons.

use super::camera::StdView;
use super::export::Format;
use super::icons::{self, Icon};
use super::render::Style;
use super::sketcher::{Outcome, Tool};
use super::*;
use crate::sketch::sketch::SkCType;
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Response, RichText, Sense, Stroke, StrokeKind};

pub mod theme {
    //! Colours measured from Creo Parametric 10's default (light) theme.
    use eframe::egui::Color32;
    pub const TITLE_BG: Color32 = Color32::from_rgb(218, 224, 224);
    pub const TAB_BG: Color32 = Color32::from_rgb(61, 70, 71);
    pub const TAB_TEXT: Color32 = Color32::from_rgb(232, 234, 234);
    pub const TAB_HOVER: Color32 = Color32::from_rgb(84, 94, 95);
    pub const RIB_TOP: Color32 = Color32::from_rgb(247, 249, 250);
    pub const RIB_BOT: Color32 = Color32::from_rgb(247, 249, 250);
    pub const BORDER: Color32 = Color32::from_rgb(178, 190, 191);
    pub const SEP: Color32 = Color32::from_rgb(196, 202, 203);
    pub const LABEL: Color32 = Color32::from_rgb(64, 64, 66);
    pub const TEXT: Color32 = Color32::from_rgb(52, 52, 54);
    pub const DIM_TEXT: Color32 = Color32::from_rgb(168, 172, 174);
    pub const HOVER_BG: Color32 = Color32::from_rgb(222, 236, 249);
    pub const HOVER_BORDER: Color32 = Color32::from_rgb(160, 198, 232);
    pub const SEL_BG: Color32 = Color32::from_rgb(204, 226, 246);
    pub const SEL_BORDER: Color32 = Color32::from_rgb(110, 162, 214);
    pub const FILE_GREEN: Color32 = Color32::from_rgb(93, 190, 60);
    pub const ACCENT: Color32 = Color32::from_rgb(70, 150, 40);
    pub const PANEL_BG: Color32 = Color32::from_rgb(247, 249, 250);
    pub const STATUS_BG: Color32 = Color32::from_rgb(247, 250, 250);
    pub const DATUM: Color32 = Color32::from_rgb(170, 129, 55);
}
use theme::*;

/// Height of a ribbon group's button area (Creo: three rows of small buttons).
pub const RIB_H: f32 = 88.0;
const FONT: f32 = 12.5;

pub fn button_bg(ui: &egui::Ui, rect: Rect, resp: &Response, enabled: bool, selected: bool) {
    let p = ui.painter();
    if selected || (enabled && resp.is_pointer_button_down_on()) {
        p.rect_filled(rect, 2.0, SEL_BG);
        p.rect_stroke(rect, 2.0, Stroke::new(1.0_f32, SEL_BORDER), StrokeKind::Inside);
    } else if enabled && resp.hovered() {
        p.rect_filled(rect, 2.0, HOVER_BG);
        p.rect_stroke(rect, 2.0, Stroke::new(1.0_f32, HOVER_BORDER), StrokeKind::Inside);
    }
}

fn text_w(ui: &egui::Ui, t: &str, f: &FontId) -> f32 { ui.fonts(|fo| fo.layout_no_wrap(t.to_string(), f.clone(), Color32::BLACK).size().x) }

/// Small downward triangle used for Creo's menu arrows.
pub fn tri(p: &egui::Painter, c: egui::Pos2, col: Color32) {
    p.add(egui::Shape::convex_polygon(vec![c + vec2(-3.5, -1.8), c + vec2(3.5, -1.8), c + vec2(0.0, 2.2)], col, Stroke::NONE));
}

/// Large ribbon button: 32 px icon with a one- or two-line label below.
pub fn large(ui: &mut egui::Ui, icon: Icon, label: &str, enabled: bool, selected: bool) -> Response {
    let font = FontId::proportional(FONT);
    let lines: Vec<&str> = label.lines().collect();
    let tw = lines.iter().map(|l| text_w(ui, l, &font)).fold(0.0f32, f32::max);
    let (rect, resp) = ui.allocate_exact_size(vec2((tw + 14.0).max(50.0), RIB_H), if enabled { Sense::click() } else { Sense::hover() });
    button_bg(ui, rect, &resp, enabled, selected);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.center().x, rect.top() + 24.0), vec2(32.0, 32.0)), icon, enabled);
    let tc = if enabled { TEXT } else { DIM_TEXT };
    for (i, l) in lines.iter().enumerate() {
        ui.painter().text(pos2(rect.center().x, rect.top() + 58.0 + i as f32 * 15.0), Align2::CENTER_CENTER, *l, font.clone(), tc);
    }
    resp
}

/// Large split button: the icon part activates, the label part (with ▾) opens a menu.
pub fn split(ui: &mut egui::Ui, icon: Icon, label: &str, enabled: bool, selected: bool) -> (Response, Response) {
    let font = FontId::proportional(FONT);
    let lines: Vec<&str> = label.lines().collect();
    let tw = lines.iter().map(|l| text_w(ui, l, &font)).fold(0.0f32, f32::max);
    let (rect, _) = ui.allocate_exact_size(vec2((tw + 14.0).max(50.0), RIB_H), Sense::hover());
    let top_r = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.top() + 46.0));
    let bot_r = Rect::from_min_max(pos2(rect.min.x, rect.top() + 46.0), rect.max);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let top = ui.interact(top_r, ui.id().with(("split_top", label)), sense);
    let bot = ui.interact(bot_r, ui.id().with(("split_bot", label)), sense);
    if selected || (enabled && (top.hovered() || bot.hovered())) {
        let any = selected || top.is_pointer_button_down_on() || bot.is_pointer_button_down_on();
        ui.painter().rect_filled(rect, 2.0, if any { SEL_BG } else { HOVER_BG });
        ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0_f32, if any { SEL_BORDER } else { HOVER_BORDER }), StrokeKind::Inside);
        if enabled { ui.painter().line_segment([pos2(rect.left() + 3.0, bot_r.top()), pos2(rect.right() - 3.0, bot_r.top())], Stroke::new(1.0_f32, HOVER_BORDER)); }
    }
    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.center().x, rect.top() + 24.0), vec2(32.0, 32.0)), icon, enabled);
    let tc = if enabled { TEXT } else { DIM_TEXT };
    for (i, l) in lines.iter().enumerate() {
        ui.painter().text(pos2(rect.center().x, rect.top() + 58.0 + i as f32 * 15.0), Align2::CENTER_CENTER, *l, font.clone(), tc);
    }
    tri(ui.painter(), pos2(rect.center().x, rect.top() + 58.0 + lines.len() as f32 * 15.0 - 2.0), tc);
    (top, bot)
}

/// Small ribbon button: 16 px icon followed by a label.
pub fn small(ui: &mut egui::Ui, icon: Icon, label: &str, enabled: bool, selected: bool) -> Response {
    let font = FontId::proportional(FONT);
    let w = text_w(ui, label, &font) + 32.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 27.0), if enabled { Sense::click() } else { Sense::hover() });
    button_bg(ui, rect, &resp, enabled, selected);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), vec2(18.0, 18.0)), icon, enabled);
    ui.painter().text(pos2(rect.left() + 27.0, rect.center().y), Align2::LEFT_CENTER, label, font, if enabled { TEXT } else { DIM_TEXT });
    resp
}

/// Small button with a ▾ that opens a menu (e.g. Rundung ▾, Fase ▾); returns (button, arrow).
pub fn small_drop(ui: &mut egui::Ui, icon: Icon, label: &str, enabled: bool, selected: bool) -> (Response, Response) {
    let font = FontId::proportional(FONT);
    let w = text_w(ui, label, &font) + 32.0;
    let (rect, _) = ui.allocate_exact_size(vec2(w + 16.0, 27.0), Sense::hover());
    let main_r = Rect::from_min_max(rect.min, pos2(rect.max.x - 16.0, rect.max.y));
    let arrow_r = Rect::from_min_max(pos2(rect.max.x - 16.0, rect.min.y), rect.max);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let main = ui.interact(main_r, ui.id().with(("sdrop", label)), sense);
    let arrow = ui.interact(arrow_r, ui.id().with(("sdrop_arrow", label)), sense);
    if enabled && (main.hovered() || arrow.hovered() || selected) {
        ui.painter().rect_filled(rect, 2.0, if selected { SEL_BG } else { HOVER_BG });
        ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0_f32, if selected { SEL_BORDER } else { HOVER_BORDER }), StrokeKind::Inside);
    }
    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), vec2(18.0, 18.0)), icon, enabled);
    let tc = if enabled { TEXT } else { DIM_TEXT };
    ui.painter().text(pos2(rect.left() + 27.0, rect.center().y), Align2::LEFT_CENTER, label, font, tc);
    tri(ui.painter(), pos2(arrow_r.center().x, rect.center().y), tc);
    (main, arrow)
}

/// Icon-only button (Quick Access Toolbar, in-graphics toolbar, mini toolbar).
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, tip: &str, enabled: bool, selected: bool, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size + 6.0, size + 6.0), if enabled { Sense::click() } else { Sense::hover() });
    button_bg(ui, rect, &resp, enabled, selected);
    icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(size, size)), icon, enabled);
    resp.on_hover_text(tip)
}

/// Icon-only ribbon button sized to one row of small buttons, optionally with a ▾.
pub fn tiny(ui: &mut egui::Ui, icon: Icon, tip: &str, enabled: bool, selected: bool, drop: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(if drop { 38.0 } else { 28.0 }, 27.0), if enabled { Sense::click() } else { Sense::hover() });
    button_bg(ui, rect, &resp, enabled, selected);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), vec2(18.0, 18.0)), icon, enabled);
    if drop { tri(ui.painter(), pos2(rect.right() - 7.0, rect.center().y + 1.0), if enabled { TEXT } else { DIM_TEXT }); }
    resp.on_hover_text(tip)
}

/// A ribbon group with its title along the bottom and a separator on the right.
pub fn group<R>(ui: &mut egui::Ui, title: &str, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    group_impl(ui, title, false, f).0
}

/// A group whose title carries Creo's ▾ overflow menu; returns the title response.
pub fn group_drop<R>(ui: &mut egui::Ui, title: &str, f: impl FnOnce(&mut egui::Ui) -> R) -> (R, Response) {
    let (r, resp) = group_impl(ui, title, true, f);
    (r, resp.expect("group title"))
}

fn group_impl<R>(ui: &mut egui::Ui, title: &str, drop: bool, f: impl FnOnce(&mut egui::Ui) -> R) -> (R, Option<Response>) {
    let mut title_resp = None;
    let out = ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
        let inner = ui.horizontal(|ui| {
            ui.set_min_height(RIB_H);
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_space(3.0);
            let r = f(ui);
            ui.add_space(3.0);
            r
        });
        let font = FontId::proportional(FONT);
        let tw = text_w(ui, title, &font) + if drop { 16.0 } else { 0.0 };
        let w = inner.response.rect.width().max(tw + 12.0);
        let (r, resp) = ui.allocate_exact_size(vec2(w, 20.0), if drop { Sense::click() } else { Sense::hover() });
        if drop && ui.memory(|m| m.is_popup_open(group_menu_id(&resp))) { ui.painter().rect_filled(r, 0.0, Color32::from_rgb(216, 222, 226)); }
        else if drop && resp.hovered() { ui.painter().rect_filled(r.shrink2(vec2(2.0, 1.0)), 2.0, HOVER_BG); }
        let c = pos2(r.center().x - if drop { 7.0 } else { 0.0 }, r.center().y);
        ui.painter().text(c, Align2::CENTER_CENTER, title, font, LABEL);
        if drop { tri(ui.painter(), pos2(c.x + tw / 2.0 - 2.0, c.y + 1.0), LABEL); title_resp = Some(resp); }
        inner.inner
    });
    let (sep, _) = ui.allocate_exact_size(vec2(5.0, RIB_H + 20.0), Sense::hover());
    ui.painter().line_segment([pos2(sep.center().x, sep.top() + 6.0), pos2(sep.center().x, sep.bottom() - 4.0)], Stroke::new(1.0_f32, SEP));
    (out.inner, title_resp)
}

/// Vertical stack of small buttons inside a group.
pub fn stack<R>(ui: &mut egui::Ui, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.add_space(2.0);
        f(ui)
    }).inner
}

/// Menu row as in Creo's drop-down menus: icon, label and a right-aligned shortcut.
pub fn menu_row(ui: &mut egui::Ui, icon: Option<Icon>, label: &str, shortcut: &str, enabled: bool, selected: bool) -> Response {
    let font = FontId::proportional(14.0);
    let w = (text_w(ui, label, &font) + 52.0 + if shortcut.is_empty() { 0.0 } else { text_w(ui, shortcut, &font) + 40.0 }).max(ui.available_width());
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 32.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if selected { p.rect_filled(rect, 0.0, SEL_BG); } else if enabled && resp.hovered() { p.rect_filled(rect, 0.0, HOVER_BG); }
    p.line_segment([pos2(rect.left() + 36.0, rect.top()), pos2(rect.left() + 36.0, rect.bottom())], Stroke::new(1.0_f32, Color32::from_rgb(226, 228, 230)));
    if let Some(ic) = icon { icons::paint(p, Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(20.0, 20.0)), ic, enabled); }
    let tc = if enabled { TEXT } else { DIM_TEXT };
    p.text(pos2(rect.left() + 46.0, rect.center().y), Align2::LEFT_CENTER, label, font.clone(), tc);
    if !shortcut.is_empty() { p.text(pos2(rect.right() - 14.0, rect.center().y), Align2::RIGHT_CENTER, shortcut, font, tc); }
    resp
}

/// Check row of Creo's filter drop-downs; `state` None draws the mixed "(Alle auswählen)" box.
pub fn check_row(ui: &mut egui::Ui, state: Option<bool>, icon: Option<Icon>, label: &str) -> bool {
    let font = FontId::proportional(14.0);
    let w = (text_w(ui, label, &font) + 84.0).max(ui.available_width());
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() { p.rect_filled(rect, 0.0, HOVER_BG); }
    p.line_segment([pos2(rect.left() + 40.0, rect.top()), pos2(rect.left() + 40.0, rect.bottom())], Stroke::new(1.0_f32, Color32::from_rgb(226, 228, 230)));
    let b = Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(15.0, 15.0));
    p.rect_filled(b, 1.0, Color32::WHITE);
    p.rect_stroke(b, 1.0, Stroke::new(1.0_f32, Color32::from_rgb(90, 96, 104)), StrokeKind::Inside);
    match state {
        Some(true) => { p.add(egui::Shape::line(vec![b.left_center() + vec2(3.0, 0.0), b.center_bottom() + vec2(-1.0, -3.5), b.right_top() + vec2(-3.0, 3.0)], Stroke::new(1.8_f32, Color32::from_rgb(40, 44, 50)))); }
        None => { p.add(egui::Shape::convex_polygon(vec![b.left_bottom() + vec2(3.0, -3.0), b.right_top() + vec2(-3.0, 3.0), b.right_bottom() + vec2(-3.0, -3.0)], Color32::from_rgb(60, 66, 74), Stroke::NONE)); }
        Some(false) => {}
    }
    let x = if let Some(ic) = icon { icons::paint(p, Rect::from_center_size(pos2(rect.left() + 60.0, rect.center().y), vec2(20.0, 20.0)), ic, true); rect.left() + 76.0 } else { rect.left() + 50.0 };
    p.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, label, font, TEXT);
    resp.clicked()
}

/// "(Alle auswählen)" plus one check row per flag.
pub fn check_list(ui: &mut egui::Ui, items: &mut [(Icon, &str, &mut bool)]) {
    let on = items.iter().filter(|i| *i.2).count();
    let all = if on == items.len() { Some(true) } else if on == 0 { Some(false) } else { None };
    if check_row(ui, all, None, "(Alle auswählen)") { let v = on != items.len(); for it in items.iter_mut() { *it.2 = v; } }
    for (ic, l, v) in items.iter_mut() { if check_row(ui, Some(**v), Some(*ic), l) { **v = !**v; } }
}

/// Creo's command tooltip: icon and bold command name, then a description.
pub fn rich_tip(resp: Response, icon: Icon, title: &str, text: &str) -> Response { cmd_tip(resp, icon, title, "", text) }

/// Creo's command tooltip with the keyboard shortcut after the name: "Rundung  (R)".
pub fn cmd_tip(resp: Response, icon: Icon, title: &str, key: &str, text: &str) -> Response {
    resp.on_hover_ui(|ui| cmd_tip_ui(ui, icon, title, key, text))
}

pub fn cmd_tip_ui(ui: &mut egui::Ui, icon: Icon, title: &str, key: &str, text: &str) {
    ui.set_max_width(400.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (r, _) = ui.allocate_exact_size(vec2(20.0, 22.0), Sense::hover());
        icons::paint(ui.painter(), r, icon, true);
        // bold name (drawn twice, half a pixel apart: the UI font has no bold face)
        let g = ui.painter().layout_no_wrap(title.to_string(), FontId::proportional(14.5), TEXT);
        let (tr, _) = ui.allocate_exact_size(g.size() + vec2(1.0, 0.0), Sense::hover());
        ui.painter().galley(tr.min, g.clone(), TEXT);
        ui.painter().galley(tr.min + vec2(0.6, 0.0), g, TEXT);
        if !key.is_empty() { ui.label(RichText::new(format!("({})", key)).size(13.5).color(TEXT)); }
    });
    ui.add_space(3.0);
    ui.label(RichText::new(text).size(13.5).color(TEXT));
}

/// Popup attached to a ribbon control (overflow menus, split-button menus).
/// FREO_DEMO_POPUP=<key> keeps that drop-down open (documentation screenshots).
pub fn demo_popup(ui: &egui::Ui, id: egui::Id, key: &str) {
    if std::env::var("FREO_DEMO_POPUP").as_deref() == Ok(key) { ui.memory_mut(|m| m.open_popup(id)); }
}

pub fn drop_menu(ui: &egui::Ui, key: &str, resp: &Response, f: impl FnOnce(&mut egui::Ui)) {
    let id = ui.id().with(("drop_menu", key));
    if resp.clicked() { ui.memory_mut(|m| m.toggle_popup(id)); }
    demo_popup(ui, id, key);
    egui::popup_below_widget(ui, id, resp, egui::PopupCloseBehavior::CloseOnClick, |ui| {
        menu_spacing(ui);
        f(ui)
    });
}

/// Popup id of the overflow menu under a group title (`group_impl` greys the title while it is open).
fn group_menu_id(title: &Response) -> egui::Id { title.id.with("group_menu") }

/// Creo's group overflow menu: hangs flush under the group title, at least as wide as the group,
/// slightly rounded and without shadow.
pub fn group_menu(ui: &mut egui::Ui, key: &str, title: &Response, f: impl FnOnce(&mut egui::Ui)) {
    let id = group_menu_id(title);
    if title.clicked() { ui.memory_mut(|m| m.toggle_popup(id)); }
    demo_popup(ui, id, key);
    ui.scope(|ui| {
        let v = ui.visuals_mut();
        v.menu_corner_radius = egui::CornerRadius::same(3);
        v.popup_shadow = egui::Shadow::NONE;
        v.window_fill = Color32::WHITE;
        v.window_stroke = Stroke::new(1.0_f32, Color32::from_rgb(200, 204, 208));
        ui.spacing_mut().menu_margin = egui::Margin::ZERO;
        egui::popup_below_widget(ui, id, title, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            menu_spacing(ui);
            ui.set_min_width(title.rect.width() - 2.0);
            f(ui)
        });
    });
}

/// Rows of every drop-down and right-click menu sit flush, as in Creo.
pub fn menu_spacing(ui: &mut egui::Ui) { ui.spacing_mut().item_spacing.y = 0.0; }

/// A menu row with an icon; the same look in every drop-down and right-click menu.
pub fn menu_entry(ui: &mut egui::Ui, icon: Icon, label: &str, enabled: bool, selected: bool) -> Response {
    menu_row(ui, Some(icon), label, "", enabled, selected)
}

/// A menu entry: icon + label (greyed when the command is not available).
pub fn menu_item(ui: &mut egui::Ui, icon: Icon, label: &str, enabled: bool) -> bool {
    menu_entry(ui, icon, label, enabled, false).clicked()
}

/// Paints the ribbon body gradient behind the current panel.
pub fn ribbon_background(ui: &egui::Ui) {
    let r = ui.max_rect().expand(8.0);
    let mut m = egui::Mesh::default();
    m.colored_vertex(r.left_top(), RIB_TOP);
    m.colored_vertex(r.right_top(), RIB_TOP);
    m.colored_vertex(r.right_bottom(), RIB_BOT);
    m.colored_vertex(r.left_bottom(), RIB_BOT);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(m));
}

fn popup_menu<R>(ui: &egui::Ui, id: egui::Id, resp: &Response, f: impl FnOnce(&mut egui::Ui) -> R) -> Option<R> {
    egui::popup_below_widget(ui, id, resp, egui::PopupCloseBehavior::CloseOnClick, |ui| {
        menu_spacing(ui);
        f(ui)
    })
}

impl App {
    pub fn context_tab_name(&self) -> Option<String> {
        if self.placing() { return Some("Komponentenplatzierung".into()); }
        if self.sketch.is_some() { return Some("Skizze".into()); }
        self.feature_tab_name()
    }

    /// Contextual tab of the feature dashboard (Creo names the Profil tab "Extrudieren").
    pub fn feature_tab_name(&self) -> Option<String> {
        let ed = self.editor.as_ref()?;
        if ed.feature.kind.is_datum() { return None; }
        if matches!(ed.feature.kind, FeatureKind::Extrude(_)) { return Some("Extrudieren".into()); }
        Some(ed.feature.kind.type_name().replace(" (Schnitt)", ""))
    }

    /// Selection filter as Creo shows it while a dashboard or the sketcher waits for input.
    pub fn filter_label(&self) -> &'static str {
        if self.creo.home { return ""; }
        if let Some(s) = &self.sketch { return s.filter.name(); }
        if self.sketch_dlg.is_some() { return "Alle"; }
        match &self.editor {
            Some(ed) if ed.pick == Some(PickTarget::SketchPlane) && ed.feature.kind.accepts_sketch() => "Skizze",
            _ => self.filter.name(),
        }
    }

    // ---------------- title bar ----------------

    pub fn title_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("titlebar").exact_height(34.0).frame(egui::Frame::NONE.fill(TITLE_BG)).show(ctx, |ui| {
            let rect = ui.max_rect();
            let drag = ui.interact(rect, egui::Id::new("titlebar_drag"), Sense::click_and_drag());
            super::ui::no_hand(ui.ctx(), drag.id);
            if drag.drag_started_by(egui::PointerButton::Primary) { ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag); }
            if drag.double_clicked() {
                let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
            }
            let within = self.editing_from_asm().and(self.asm.as_ref()).map(|a| format!("  ⟵ {}.ASM", a.data.name)).unwrap_or_default();
            let title = if self.creo.home { "Freo".to_string() } else { match self.drw.as_ref().filter(|d| d.active) {
                Some(d) => format!("{} (Aktiv){} - Freo", d.data.name, if d.dirty { " *" } else { "" }),
                None => format!("{} (Aktiv){}{} - Freo", self.data.name, if self.dirty { " *" } else { "" }, within),
            } };
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, title, FontId::proportional(14.0), TEXT);
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.add_space(78.0);
                // product mark in the spot of Creo's logo
                let (lr, _) = ui.allocate_exact_size(vec2(72.0, 24.0), Sense::hover());
                let sq = Rect::from_min_size(pos2(lr.left() + 2.0, lr.center().y - 7.0), vec2(14.0, 14.0));
                ui.painter().rect_stroke(sq, 2.0, Stroke::new(2.0_f32, Color32::from_rgb(52, 120, 190)), StrokeKind::Inside);
                ui.painter().rect_filled(Rect::from_min_size(sq.min + vec2(3.5, 3.5), vec2(5.0, 7.0)), 0.0, Color32::from_rgb(52, 120, 190));
                ui.painter().text(pos2(sq.right() + 5.0, lr.center().y), Align2::LEFT_CENTER, "freo", FontId::proportional(17.0), Color32::from_rgb(70, 76, 80));
                ui.add_space(6.0);
                let busy = self.busy();
                let home = self.creo.home;
                if icon_button(ui, Icon::New, "Neu (⌘N)", true, false, 18.0).clicked() { self.confirm_new(); }
                if icon_button(ui, Icon::Save, "Speichern (⌘S)", !home, false, 18.0).clicked() { self.save(false); }
                if icon_button(ui, Icon::Export, "Kopie speichern / Exportieren (STL, OBJ, STEP, 3MF ...) ⇧⌘E", !home && !self.drawing_active(), false, 18.0).clicked() { self.export_dialog(None); }
                let r = icon_button(ui, Icon::Open, "Öffnen (⌘O)", true, false, 18.0);
                if r.clicked() { self.open(); }
                let drw = self.drw.as_ref().filter(|d| d.active).map(|d| (!d.undo.is_empty(), !d.redo.is_empty()));
                let (cu, cr) = if home { (false, false) } else { match (&self.sketch, self.asm.as_ref().filter(|a| a.editing.is_none()), drw) {
                    (_, _, Some(x)) => x,
                    (Some(s), _, _) => (s.can_undo(), s.can_redo()),
                    (None, Some(a), _) => (!a.undo.is_empty() && !busy, !a.redo.is_empty() && !busy),
                    (None, None, _) => (!self.undo.is_empty() && !busy, !self.redo.is_empty() && !busy),
                } };
                if icon_button(ui, Icon::Undo, "Rückgängig (⌘Z)", cu, false, 18.0).clicked() { if drw.is_some() { self.drw_undo(false) } else if let Some(s) = &mut self.sketch { s.undo() } else { self.do_undo() } }
                if icon_button(ui, Icon::Redo, "Wiederholen (⇧⌘Z)", cr, false, 18.0).clicked() { if drw.is_some() { self.drw_undo(true) } else if let Some(s) = &mut self.sketch { s.redo() } else { self.do_redo() } }
                if icon_button(ui, Icon::Regenerate, "Regenerieren (⌘G)", !home, false, 18.0).clicked() { if drw.is_some() { if let Some(d) = &mut self.drw { d.cache.clear(); } } else { self.regenerate(true); } }
                icon_button(ui, Icon::Windows, "Fenster", false, false, 18.0);
                if icon_button(ui, Icon::Close, "Schließen", !home, false, 18.0).clicked() { self.close_model(); }
                let (cr_, _) = ui.allocate_exact_size(vec2(16.0, 22.0), Sense::hover());
                tri(ui.painter(), cr_.center(), TEXT);
            });
        });
    }

    /// File ▸ New: Creo's New dialog (Part or Assembly).
    pub fn confirm_new(&mut self) {
        if self.busy() { self.say("Zuerst das aktive KE abschließen.", MsgKind::Warning); return; }
        let asm = self.in_asm();
        let drw = self.drawing_active();
        self.new_dialog = Some(super::assembly::NewDialog { assembly: asm && !drw, drawing: drw, name: if drw { "drw0002".into() } else if asm { "asm0001".into() } else { "prt0001".into() }, common: String::new(), template: true });
    }

    /// File ▸ Close: closes the drawing, returns to the assembly from an opened component,
    /// otherwise starts over.
    pub fn close_model(&mut self) {
        if self.drawing_active() { self.close_drawing(); return; }
        if self.editing_from_asm().is_some() { self.asm_return(); return; }
        if self.busy() { self.say("Zuerst das aktive KE abschließen.", MsgKind::Warning); return; }
        if self.confirm_discard() { self.go_home(); }
    }

    // ---------------- tab strip ----------------

    /// Contextual tab (Extrudieren, Skizze, ...): italic label as in Creo; `parent` is the feature
    /// tab shown greyed out next to Skizze while its sketch is open.
    fn ctx_tab_button(ui: &mut egui::Ui, label: &str, active: bool, parent: bool, pad: f32) -> Response {
        let font = FontId::proportional(14.0);
        let col = if active { Color32::from_rgb(55, 60, 62) } else if parent { Color32::from_rgb(105, 114, 120) } else { Color32::from_rgb(150, 214, 120) };
        let mut job = egui::text::LayoutJob::default();
        job.append(label, 0.0, egui::TextFormat { font_id: font, color: col, italics: true, ..Default::default() });
        let galley = ui.fonts(|f| f.layout_job(job));
        let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + pad, 32.0), if parent { Sense::hover() } else { Sense::click() });
        let p = ui.painter();
        if active { p.rect_filled(rect, 0.0, RIB_TOP); }
        else if parent { p.rect_filled(rect, 0.0, Color32::from_rgb(196, 202, 205)); }
        else if resp.hovered() { p.rect_filled(rect, 0.0, TAB_HOVER); }
        p.galley(rect.center() - galley.size() / 2.0, galley, col);
        resp
    }

    fn tab_button(ui: &mut egui::Ui, label: &str, active: bool, accent: bool, pad: f32) -> Response {
        let font = FontId::proportional(14.0);
        let w = text_w(ui, label, &font) + pad;
        let (rect, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
        let p = ui.painter();
        if active {
            p.rect_filled(rect, 0.0, RIB_TOP);
        } else if resp.hovered() {
            p.rect_filled(rect, 0.0, TAB_HOVER);
        }
        let col = if active { ACCENT } else if accent { Color32::from_rgb(150, 214, 120) } else { TAB_TEXT };
        p.text(rect.center(), Align2::CENTER_CENTER, label, font, col);
        resp
    }

    /// Tabs of the current mode, as in Creo 10 (Startseite without a model).
    pub fn tab_list(&self) -> Vec<(Tab, &'static str)> {
        if self.creo.home { return vec![(Tab::Home, "Startseite")]; }
        if self.drawing_active() { return vec![(Tab::Layout, "Layout"), (Tab::Table, "Tabelle"), (Tab::Annotate, "Anmerkungen erstellen"), (Tab::Publish, "Veröffentlichen")]; }
        vec![(Tab::Model, "Modell"), (Tab::Analysis, "Analyse"), (Tab::LiveSim, "Live-Simulation"), (Tab::Annot3d, "Anmerkungen erstellen"), (Tab::Tools, "Werkzeuge"),
             (Tab::View, "Ansicht"), (Tab::Flex, "Flexible Modellierung"), (Tab::Apps, "Anwendungen"), (Tab::Htl, "HTL")]
    }

    pub fn tab_strip(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("tabs").exact_height(32.0).frame(egui::Frame::NONE.fill(TAB_BG)).show(ctx, |ui| {
            let full = ui.max_rect();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let (r, resp) = ui.allocate_exact_size(vec2(78.0, 32.0), Sense::click());
                ui.painter().rect_filled(r, 0.0, if resp.hovered() || self.file_menu { Color32::from_rgb(110, 202, 78) } else { FILE_GREEN });
                ui.painter().text(r.center(), Align2::CENTER_CENTER, "Datei", FontId::proportional(14.0), Color32::WHITE);
                if resp.clicked() { self.file_menu = !self.file_menu; }
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("file_anchor"), pos2(r.left(), full.bottom())));
                let tabs = self.tab_list();
                if !tabs.iter().any(|(t, _)| *t == self.tab) && self.tab != Tab::Context { self.tab = tabs[0].0; }
                let ctx_name = if self.creo.home { None } else if self.drawing_active() { self.drawing_context_name().map(|s| s.to_string()) } else { self.context_tab_name() };
                let parent_tab = if ctx_name.is_some() && self.sketch.is_some() && !self.creo.home && !self.drawing_active() { self.feature_tab_name() } else { None };
                // like Creo, tabs get narrower when the contextual tabs need the room
                let font = FontId::proportional(14.0);
                let labels: Vec<&str> = tabs.iter().map(|t| t.1).chain(ctx_name.as_deref()).chain(parent_tab.as_deref()).collect();
                let text: f32 = labels.iter().map(|l| text_w(ui, l, &font)).sum();
                let pad = ((full.width() - 78.0 - 150.0 - text) / labels.len() as f32).clamp(12.0, 44.0);
                for (t, n) in &tabs {
                    if Self::tab_button(ui, n, self.tab == *t && !self.creo.ribbon_min_hidden(), false, pad).clicked() {
                        if self.creo.ribbon_min { self.creo.peek = !(self.tab == *t && self.creo.peek); } 
                        self.tab = *t;
                    }
                }
                if let Some(n) = ctx_name {
                    if let Some(parent) = &parent_tab { Self::ctx_tab_button(ui, parent, false, true, pad); }
                    if Self::ctx_tab_button(ui, &n, self.tab == Tab::Context, false, pad).clicked() { self.tab = Tab::Context; }
                } else if self.tab == Tab::Context {
                    self.tab = tabs[0].0;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.add_space(8.0);
                    let white = Color32::from_rgb(236, 238, 238);
                    let chrome = |ui: &mut egui::Ui, tip: &str, draw: &dyn Fn(&egui::Painter, egui::Pos2)| -> Response {
                        let (r, resp) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
                        if resp.hovered() { ui.painter().rect_filled(r.shrink(2.0), 2.0, TAB_HOVER); }
                        draw(ui.painter(), r.center());
                        resp.on_hover_text(tip)
                    };
                    if chrome(ui, "Hilfe", &|p, c| { p.text(c, Align2::CENTER_CENTER, "?", FontId::proportional(17.0), white); }).clicked() { self.windows.controls = true; }
                    chrome(ui, "Anpassen", &|p, c| tri(p, c, white));
                    if chrome(ui, "Suchen", &|p, c| {
                        p.circle_stroke(c + vec2(-2.0, -2.0), 5.0, Stroke::new(2.0_f32, white));
                        p.line_segment([c + vec2(1.8, 1.8), c + vec2(6.5, 6.5)], Stroke::new(2.4_f32, white));
                    }).clicked() { self.creo.search = Some(String::new()); }
                    let min = self.creo.ribbon_min;
                    if chrome(ui, if min { "Menüband maximieren" } else { "Menüband minimieren" }, &|p, c| {
                        let pts = if min { [c + vec2(-6.0, -3.0), c + vec2(0.0, 3.0), c + vec2(6.0, -3.0)] } else { [c + vec2(-6.0, 3.0), c + vec2(0.0, -3.0), c + vec2(6.0, 3.0)] };
                        p.add(egui::Shape::line(pts.to_vec(), Stroke::new(2.4_f32, white)));
                    }).clicked() { self.creo.ribbon_min = !min; self.creo.peek = false; }
                });
            });
        });
    }

    // ---------------- File menu ----------------

    fn file_item(ui: &mut egui::Ui, icon: Icon, label: &str, enabled: bool, indent: f32) -> Response {
        let (rect, resp) = ui.allocate_exact_size(vec2(210.0, if indent > 0.0 { 26.0 } else { 36.0 }), if enabled { Sense::click() } else { Sense::hover() });
        if enabled && resp.hovered() {
            ui.painter().rect_filled(rect.shrink(1.0), 2.0, HOVER_BG);
            ui.painter().rect_stroke(rect.shrink(1.0), 2.0, Stroke::new(1.0_f32, HOVER_BORDER), StrokeKind::Inside);
        }
        let isz = if indent > 0.0 { 16.0 } else { 24.0 };
        icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 20.0 + indent, rect.center().y), vec2(isz, isz)), icon, enabled);
        ui.painter().text(pos2(rect.left() + 40.0 + indent, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(if indent > 0.0 { 12.0 } else { 13.5 }), if enabled { TEXT } else { DIM_TEXT });
        resp
    }

    pub fn file_menu_ui(&mut self, ctx: &egui::Context) {
        if !self.file_menu { return; }
        let anchor: egui::Pos2 = ctx.data(|d| d.get_temp(egui::Id::new("file_anchor"))).unwrap_or(pos2(6.0, 56.0));
        let mut close = false;
        let area = egui::Area::new(egui::Id::new("file_menu")).fixed_pos(anchor).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(Color32::WHITE).inner_margin(0.0).corner_radius(3.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    egui::Frame::NONE.fill(PANEL_BG).inner_margin(6.0).show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(212.0);
                            ui.spacing_mut().item_spacing.y = 1.0;
                            if Self::file_item(ui, Icon::New, "Neu", true, 0.0).clicked() { close = true; self.confirm_new(); }
                            if Self::file_item(ui, Icon::Open, "Öffnen...", true, 0.0).clicked() { close = true; self.open(); }
                            let model = !self.creo.home;
                            if Self::file_item(ui, Icon::Save, "Speichern", model, 0.0).clicked() { close = true; self.save(false); }
                            if Self::file_item(ui, Icon::SaveAs, "Speichern unter...", model, 0.0).clicked() { close = true; self.save(true); }
                            if !self.drawing_active() && Self::file_item(ui, Icon::Export, "Kopie speichern / Exportieren...", model, 0.0).on_hover_text("STL, OBJ, STEP, PLY, VRML, glTF, 3MF").clicked() { close = true; self.export_dialog(None); }
                            ui.separator();
                            ui.label(egui::RichText::new("   Direkt exportieren als").size(11.0).color(LABEL));
                            if self.drawing_active() {
                                for (icon, label, kind) in [(Icon::Pdf, "PDF (alle Blätter)", "pdf"), (Icon::Dxf, "DXF (Blatt)", "dxf"), (Icon::Svg, "SVG (Blatt)", "svg")] {
                                    if Self::file_item(ui, icon, label, true, 16.0).clicked() { close = true; self.export_drawing(kind); }
                                }
                            } else {
                                for f in Format::ALL {
                                    if Self::file_item(ui, Icon::Export, &format!("{}  (.{})", f.name(), f.ext()), model, 16.0).on_hover_text(f.hint()).clicked() { close = true; self.export_dialog(Some(f)); }
                                }
                            }
                            ui.separator();
                            if self.drw.as_ref().map(|d| !d.active).unwrap_or(false) {
                                if Self::file_item(ui, Icon::ReturnDwg, "Zurück zur Zeichnung", true, 0.0).clicked() { close = true; self.drawing_return(); }
                            }
                            if self.editing_from_asm().is_some() {
                                if Self::file_item(ui, Icon::ReturnAsm, "Zurück zur Baugruppe", true, 0.0).clicked() { close = true; self.asm_return(); }
                            }
                            if Self::file_item(ui, Icon::Close, "Schließen", model, 0.0).clicked() { close = true; self.close_model(); }
                            if Self::file_item(ui, Icon::Exit, "Beenden", true, 0.0).clicked() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
                        });
                    });
                    egui::Frame::NONE.inner_margin(12.0).show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(300.0);
                            ui.label(egui::RichText::new("Zuletzt geöffnete Dateien").strong().size(13.0));
                            ui.separator();
                            if self.recent.is_empty() { ui.label(egui::RichText::new("Keine zuletzt geöffneten Dateien").color(LABEL)); }
                            let recent = self.recent.clone();
                            for (i, p) in recent.iter().enumerate() {
                                let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                                let r = ui.add(egui::Button::new(egui::RichText::new(format!("{}  {}", i + 1, name)).size(12.5)).frame(false)).on_hover_text(p.display().to_string());
                                if r.clicked() { close = true; if self.confirm_discard() { self.load(p.clone()); } }
                            }
                        });
                    });
                });
            });
        });
        let clicked_outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().map(|p| !area.response.rect.contains(p) && p.y > anchor.y).unwrap_or(false));
        if close || clicked_outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) { self.file_menu = false; }
    }

    // ---------------- ribbons ----------------

    pub fn ribbon_body(&mut self, ui: &mut egui::Ui) -> Outcome {
        ribbon_background(ui);
        let mut out = Outcome::None;
        if self.tab == Tab::Context && self.sketch.is_some() {
            // the Close group stays pinned to the right edge like Creo's OK/Cancel
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let w = (ui.available_width() - 150.0).max(200.0);
                egui::ScrollArea::horizontal().id_salt("sketch_ribbon").max_width(w).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                    ui.horizontal(|ui| { ui.spacing_mut().item_spacing.x = 0.0; self.sketch_ribbon(ui); });
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| { out = self.sketch_close(ui); });
            });
            return out;
        }
        egui::ScrollArea::horizontal().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let asm = self.in_asm();
                match self.tab {
                    Tab::Home => self.home_ribbon(ui),
                    Tab::Model if asm => self.asm_model_ribbon(ui),
                    Tab::Model => self.model_ribbon(ui),
                    Tab::Analysis if asm => self.asm_analysis_ribbon(ui),
                    Tab::Analysis => self.analysis_ribbon(ui),
                    Tab::LiveSim => self.live_sim_ribbon(ui),
                    Tab::Annot3d => self.annot3d_ribbon(ui),
                    Tab::View => self.view_ribbon(ui),
                    Tab::Tools if asm => self.asm_tools_ribbon(ui),
                    Tab::Tools => self.tools_ribbon(ui),
                    Tab::Flex => self.flex_ribbon(ui),
                    Tab::Apps => self.apps_ribbon(ui),
                    Tab::Htl => self.htl_ribbon(ui),
                    Tab::Layout | Tab::Table | Tab::Annotate | Tab::Publish => { self.tab = Tab::Model; }
                    Tab::Context => {
                        if self.placing() { self.placement_ribbon(ui); }
                        else if self.editor.is_some() { self.feature_ribbon(ui); }
                        else if asm { self.asm_model_ribbon(ui); }
                        else { self.model_ribbon(ui); }
                    }
                }
            });
        });
        out
    }

    /// Ribbon group shown in 3D while a drawing of the model waits in the background.
    pub fn return_to_drawing_group(&mut self, ui: &mut egui::Ui) {
        let Some(name) = self.drw.as_ref().filter(|d| !d.active).map(|d| d.data.name.clone()) else { return };
        let free = !self.busy();
        group(ui, "Zeichnung", |ui| {
            if large(ui, Icon::ReturnDwg, "Zurück zur\nZeichnung", free, false).on_hover_text(format!("{}.DRW wieder anzeigen (ihre Ansichten folgen dem Modell)", name)).clicked() { self.drawing_return(); }
        });
    }

    /// Model ▸ Operations, shared by parts and assemblies.
    pub fn operations_group(&mut self, ui: &mut egui::Ui) {
        let free = self.editor.is_none() && self.sketch.is_none() && !self.placing();
        let sel = !self.tree_selection.is_empty() && free;
        let asm = self.in_asm();
        let ((), title) = group_drop(ui, "Operationen", |ui| {
            let (top, bot) = split(ui, Icon::Regenerate, "Regenerieren", true, false);
            if top.clicked() { if asm { self.asm_regenerate(true) } else { self.regenerate(true) } }
            drop_menu(ui, "regen", &bot, |ui| {
                if menu_item(ui, Icon::Regenerate, "Regenerieren", true) { if asm { self.asm_regenerate(true) } else { self.regenerate(true) } }
                if menu_item(ui, Icon::Settings, "Regenerierungsmanager", !asm) { self.creo.regen_manager = true; }
            });
            stack(ui, |ui| {
                if tiny(ui, Icon::Copy, "Kopieren", sel && !asm, false, false).clicked() { self.copy_features(); }
                let pr = tiny(ui, Icon::Paste, "Einfügen", free && !asm && !self.creo.clip_features.is_empty(), false, true);
                if pr.clicked() { self.paste_features(); }
                let r = tiny(ui, Icon::Delete, "Löschen", sel, false, true);
                if r.clicked() { let ids = self.tree_selection.clone(); if asm { self.asm_delete(&ids) } else { self.delete_features(&ids) } }
            });
        });
        group_menu(ui, "ops", &title, |ui| {
            if menu_item(ui, Icon::Undo, "Rückgängig", !self.undo.is_empty() && free && !asm) { self.do_undo(); }
            if menu_item(ui, Icon::Redo, "Wiederholen", !self.redo.is_empty() && free && !asm) { self.do_redo(); }
            if menu_item(ui, Icon::Suppress, "Unterdrücken", sel && !asm) { let ids = self.tree_selection.clone(); self.ask_suppress(&ids); }
            if menu_item(ui, Icon::Resume, "Fortsetzen", sel && !asm) { let ids = self.tree_selection.clone(); self.set_suppressed(&ids, false); }
        });
    }

    fn model_ribbon(&mut self, ui: &mut egui::Ui) {
        let free = self.editor.is_none() && self.sketch.is_none();
        // like Creo, engineering features need solid geometry to attach to
        let solid = free && !self.result.topo.polys.is_empty();
        self.return_to_drawing_group(ui);
        if let Some(name) = self.editing_from_asm().and(self.asm.as_ref()).map(|a| a.data.name.clone()) {
            group(ui, "Baugruppe", |ui| {
                if large(ui, Icon::ReturnAsm, "Zurück zur\nBaugruppe", free, false).on_hover_text(format!("Das Teil in {}.ASM speichern und die Baugruppe anzeigen", name)).clicked() { self.asm_return(); }
            });
        }
        self.operations_group(ui);
        let ((), t) = group_drop(ui, "Daten abrufen", |ui| {
            stack(ui, |ui| {
                if tiny(ui, Icon::GetData, "Importieren (STEP, STL, OBJ)", free, false, false).clicked() { self.import_dialog(); }
                if tiny(ui, Icon::UdfLib, "Benutzerdefiniertes KE", free, false, false).clicked() { self.open_file_task(super::modelcmds::FileTask::Udf); }
                if tiny(ui, Icon::PublishGeom, "Shrinkwrap", free, false, false).clicked() { self.open_file_task(super::modelcmds::FileTask::Shrinkwrap); }
            });
        });
        group_menu(ui, "getdata", &t, |ui| {
            if menu_item(ui, Icon::GetData, "Importieren (STEP, STL, OBJ)...", free) { self.import_dialog(); }
            if menu_item(ui, Icon::Copy, "Kopiergeometrie", free) { self.open_file_task(super::modelcmds::FileTask::CopyGeom); }
            if menu_item(ui, Icon::UdfLib, "Benutzerdefiniertes KE", free) { self.open_file_task(super::modelcmds::FileTask::Udf); }
            if menu_item(ui, Icon::PublishGeom, "Shrinkwrap", free) { self.open_file_task(super::modelcmds::FileTask::Shrinkwrap); }
        });
        let ((), t) = group_drop(ui, "Körper", |ui| {
            stack(ui, |ui| {
                if tiny(ui, Icon::Body, "Körper", free, false, false).on_hover_text("Einen neuen Körper erzeugen; neue KEs kommen in ihn").clicked() { self.new_body(); }
                if tiny(ui, Icon::Merge, "Boolesche Operationen", free, false, false).on_hover_text("Zwei Körper vereinigen, subtrahieren oder schneiden").clicked() { self.start_feature(FeatureKind::new_body(BodyOp::Union)); }
                if tiny(ui, Icon::Trim, "Körper teilen", free, false, false).on_hover_text("Einen Körper an einer Ebene in zwei Körper teilen").clicked() { self.start_feature(FeatureKind::new_body(BodyOp::Split)); }
            });
        });
        group_menu(ui, "bodies", &t, |ui| {
            if menu_item(ui, Icon::Body, "Neuer Körper", free) { self.new_body(); }
            if menu_item(ui, Icon::Merge, "Boolesche Operationen", free) { self.start_feature(FeatureKind::new_body(BodyOp::Union)); }
            if menu_item(ui, Icon::Trim, "Körper teilen", free) { self.start_feature(FeatureKind::new_body(BodyOp::Split)); }
        });
        let ((), t) = group_drop(ui, "Bezug", |ui| {
            if large(ui, Icon::Plane, "Ebene", free, false).on_hover_text("Eine Bezugsebene erzeugen").clicked() { self.start_feature(FeatureKind::new_plane()); }
            stack(ui, |ui| {
                if tiny(ui, Icon::Axis, "Achse", free, false, false).clicked() { self.start_feature(FeatureKind::new_axis()); }
                if tiny(ui, Icon::Point, "Punkt", free, false, true).clicked() { self.start_feature(FeatureKind::new_point()); }
                if tiny(ui, Icon::Csys, "Koordinatensystem", free, false, false).on_hover_text("Ein Koordinatensystem erzeugen").clicked() { self.start_feature(FeatureKind::new_csys()); }
            });
            if large(ui, Icon::Sketch, "Skizze", free, false).on_hover_text("Eine skizzierte Bezugskurve erzeugen").clicked() { self.start_feature(FeatureKind::new_sketch()); }
        });
        group_menu(ui, "datum", &t, |ui| {
            if menu_item(ui, Icon::Sketch, "Kurve durch Skizze", free) { self.start_feature(FeatureKind::new_sketch()); }
            if menu_item(ui, Icon::CurvePoints, "Kurve durch Punkte", free) { self.start_feature(FeatureKind::new_curve()); }
        });
        let ((), t) = group_drop(ui, "Formen", |ui| {
            if rich_tip(large(ui, Icon::Extrude, "Profil", free, false), Icon::Extrude, "Profil", "Dreidimensionale Geometrie durch Projizieren eines zweidimensionalen Schnitts in einem angegebenen Abstand senkrecht zur Skizzierebene erzeugen.").clicked() { self.start_feature(FeatureKind::new_extrude()); }
            stack(ui, |ui| {
                if small(ui, Icon::Revolve, "Drehen", free, false).on_hover_text("Ein Rotations-KE erzeugen").clicked() { self.start_feature(FeatureKind::new_revolve()); }
                let (m, arr) = small_drop(ui, Icon::Sweep, "Zug-KE", free, false);
                if m.clicked() { self.start_feature(FeatureKind::new_sweep()); }
                drop_menu(ui, "sweep", &arr, |ui| {
                    if menu_item(ui, Icon::Sweep, "Zug-KE", free) { self.start_feature(FeatureKind::new_sweep()); }
                    if menu_item(ui, Icon::HelicalSweep, "Spiralzug-KE", free) { self.start_feature(FeatureKind::new_helix()); }
                    if menu_item(ui, Icon::Sweep, "Volumenzug-KE", free) { self.start_sweep(SweepMode::Volume); }
                });
                if small(ui, Icon::SweptBlend, "Zug-Verbund", free, false).on_hover_text("Ein Querschnitt, der entlang der Leitkurve in einen zweiten übergeht").clicked() { self.start_sweep(SweepMode::Blend); }
            });
        });
        group_menu(ui, "shapes", &t, |ui| {
            if menu_item(ui, Icon::Blend, "Verbund", free) { self.start_feature(FeatureKind::new_blend()); }
            if menu_item(ui, Icon::RotBlend, "Rotationsverbund", free) { self.start_feature(FeatureKind::new_rotational_blend()); }
        });
        let ((), t) = group_drop(ui, "Konstruktion", |ui| {
            stack(ui, |ui| {
                if small(ui, Icon::Hole, "Bohrung", solid, false).on_hover_text("Eine Bohrung erzeugen").clicked() { self.start_feature(FeatureKind::new_hole()); }
                let (m, arr) = small_drop(ui, Icon::Round, "Rundung", solid, false);
                if m.clicked() { self.start_feature(FeatureKind::new_round()); }
                drop_menu(ui, "round", &arr, |ui| {
                    if menu_item(ui, Icon::Round, "Rundung", solid) { self.start_feature(FeatureKind::new_round()); }
                    if menu_item(ui, Icon::Round, "Automatische Rundung", solid) { self.auto_round(); }
                });
                let (m, arr) = small_drop(ui, Icon::Chamfer, "Fase", solid, false);
                if m.clicked() { self.start_feature(FeatureKind::new_chamfer()); }
                drop_menu(ui, "chamfer", &arr, |ui| {
                    if menu_item(ui, Icon::Chamfer, "Kantenfase", solid) { self.start_feature(FeatureKind::new_chamfer()); }
                    if menu_item(ui, Icon::Chamfer, "Eckfase", solid) { let mut k = FeatureKind::new_chamfer(); if let FeatureKind::Chamfer(c) = &mut k { c.corner = true; } self.start_feature(k); self.say("Eckfase: zwei Kanten wählen, die sich in der Ecke treffen.", MsgKind::Prompt); }
                });
            });
            stack(ui, |ui| {
                let (m, arr) = small_drop(ui, Icon::Draft, "Schräge", solid, false);
                if m.clicked() { self.start_feature(FeatureKind::new_draft()); }
                drop_menu(ui, "draft", &arr, |ui| {
                    if menu_item(ui, Icon::Draft, "Schräge", solid) { self.start_feature(FeatureKind::new_draft()); }
                    if menu_item(ui, Icon::Draft, "Variable Zugschräge", solid) { let mut k = FeatureKind::new_draft(); if let FeatureKind::Draft(c) = &mut k { c.angle2 = Some(Dim::v(10.0)); } self.start_feature(k); }
                });
                if small(ui, Icon::Shell, "Schale", solid, false).clicked() { self.start_feature(FeatureKind::new_shell()); }
                let (m, arr) = small_drop(ui, Icon::Rib, "Rippe", solid, false);
                if m.on_hover_text("Eine Profilrippe erzeugen").clicked() { self.start_feature(FeatureKind::new_rib()); }
                drop_menu(ui, "rib", &arr, |ui| { if menu_item(ui, Icon::Rib, "Profilrippe", solid) { self.start_feature(FeatureKind::new_rib()); } });
            });
        });
        group_menu(ui, "engineering", &t, |ui| { if menu_item(ui, Icon::CosmThread, "Kosmetisches Gewinde", solid) { self.start_feature(FeatureKind::new_thread()); } });
        let ((), t) = group_drop(ui, "Editieren", |ui| {
            let (top, bot) = split(ui, Icon::Pattern, "Muster", free, false);
            if top.clicked() { self.start_feature(FeatureKind::new_pattern()); }
            drop_menu(ui, "pattern", &bot, |ui| {
                if menu_item(ui, Icon::Pattern, "Muster", free) { self.start_feature(FeatureKind::new_pattern()); }
                if menu_item(ui, Icon::Pattern, "Geometriemuster", free) { self.geometry_pattern(); }
                if menu_item(ui, Icon::FamilyTable, "Mustertabelle bearbeiten", free) { self.pattern_table(); }
            });
            let cells: [[(Icon, &str, Option<SurfOp>); 4]; 3] = [
                [(Icon::Mirror, "Spiegeln", None), (Icon::Extend, "Verlängern", Some(SurfOp::Extend)), (Icon::Project3d, "Projizieren", Some(SurfOp::Project)), (Icon::Offset, "Versatz", Some(SurfOp::Offset))],
                [(Icon::Merge, "Zusammenführen", Some(SurfOp::Merge)), (Icon::Trim, "Trimmen", Some(SurfOp::Trim)), (Icon::Intersect, "Schnittpunkt", Some(SurfOp::Intersect)), (Icon::Thicken, "Aufdicken", Some(SurfOp::Thicken))],
                [(Icon::Solidify, "Volumenkörper erzeugen", Some(SurfOp::Solidify)), (Icon::BoundaryBlend, "Teilen", Some(SurfOp::Split)), (Icon::FillSurf, "Ersetzen", Some(SurfOp::Replace)), (Icon::Copy, "Bearbeitbare Kopie", Some(SurfOp::Copy))],
            ];
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add_space(2.0);
                for row in cells {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 1.0;
                        for (ic, tip, op) in row {
                            if tiny(ui, ic, tip, free, false, false).clicked() {
                                match op { Some(op) => self.start_feature(FeatureKind::new_surface(op)), None => self.start_feature(FeatureKind::new_mirror()) }
                            }
                        }
                    });
                }
            });
        });
        group_menu(ui, "edit", &t, |ui| { if menu_item(ui, Icon::Mirror, "Spiegeln", free) { self.start_feature(FeatureKind::new_mirror()); } });
        let ((), t) = group_drop(ui, "Flächen", |ui| {
            if large(ui, Icon::BoundaryBlend, "Berandungsverbund", free, false).on_hover_text("Eine Fläche zwischen zwei oder vier Randkurven").clicked() { self.start_feature(FeatureKind::new_surface(SurfOp::Boundary)); }
            stack(ui, |ui| {
                if tiny(ui, Icon::FillSurf, "Füllen", free, false, false).clicked() { self.start_feature(FeatureKind::new_surface(SurfOp::Fill)); }
                if tiny(ui, Icon::Style, "Stil", free, false, false).clicked() { self.start_feature(FeatureKind::new_surface(SurfOp::Style)); }
                if tiny(ui, Icon::Freestyle, "Freistil", free, false, false).clicked() { self.start_feature(FeatureKind::new_surface(SurfOp::Freestyle)); }
            });
        });
        group_menu(ui, "surfaces", &t, |ui| {
            for (ic, op) in [(Icon::FillSurf, SurfOp::Fill), (Icon::BoundaryBlend, SurfOp::Boundary), (Icon::Style, SurfOp::Style), (Icon::Freestyle, SurfOp::Freestyle)] {
                if menu_item(ui, ic, op.name(), free) { self.start_feature(FeatureKind::new_surface(op)); }
            }
        });
        let ((), t) = group_drop(ui, "Modellabsicht", |ui| {
            if large(ui, Icon::CompInterface, "Komponentenschnittstelle", !self.in_asm(), self.creo.iface_dialog).on_hover_text("Platzierungsreferenzen festlegen, die das Teil in Baugruppen mitbringt").clicked() { self.creo.iface_dialog = !self.creo.iface_dialog; }
        });
        group_menu(ui, "intent", &t, |ui| {
            if menu_item(ui, Icon::Parameters, "Parameter", true) { self.windows.parameters = true; }
            if menu_item(ui, Icon::Relations, "Beziehungen", true) { self.windows.relations = true; }
            if menu_item(ui, Icon::FamilyTable, "Familientabelle", true) { self.windows.family = true; }
        });
    }

    fn analysis_ribbon(&mut self, ui: &mut egui::Ui) {
        let ((), t) = group_drop(ui, "Verwalten", |ui| {
            stack(ui, |ui| {
                tiny(ui, Icon::GeomReport, "Gespeicherte Analysen", false, false, false);
                tiny(ui, Icon::Save, "Analyse speichern", false, false, false);
                tiny(ui, Icon::Curvature, "Alle ausblenden", false, false, false);
            });
        });
        group_menu(ui, "manage", &t, |ui| { menu_item(ui, Icon::Delete, "Alle temporären löschen", false); });
        group(ui, "Benutzerdefiniert", |ui| {
            stack(ui, |ui| {
                tiny(ui, Icon::FamilyTable, "Excel-Analyse", false, false, false);
                tiny(ui, Icon::FamilyTable, "Mathcad-Analyse", false, false, false);
                tiny(ui, Icon::DimBound, "Benutzerdefinierte Analyse", false, false, false);
            });
            stack(ui, |ui| {
                tiny(ui, Icon::Mathcad, "Mathcad", false, false, false);
                tiny(ui, Icon::Mathcad, "Mathcad-Datei", false, false, false);
                tiny(ui, Icon::Probe, "Prüfstandsanalyse", false, false, false);
            });
        });
        let ((), t) = group_drop(ui, "Modellbericht", |ui| {
            stack(ui, |ui| {
                let (m, arr) = small_drop(ui, Icon::MassProps, "Masseneigenschaften", true, self.windows.mass);
                if m.clicked() { self.windows.mass = !self.windows.mass; }
                drop_menu(ui, "mass", &arr, |ui| {
                    if menu_item(ui, Icon::MassProps, "Masseneigenschaften", true) { self.windows.mass = true; }
                    if menu_item(ui, Icon::ModelInfo, "Modellinfo", true) { self.windows.info = true; }
                });
                small(ui, Icon::ShortEdge, "Kurze Kante", false, false);
                small(ui, Icon::Thickness, "Dicke", false, false);
            });
        });
        group_menu(ui, "report", &t, |ui| { if menu_item(ui, Icon::ModelInfo, "Modellinfo", true) { self.windows.info = true; } });
        group(ui, "Messen", |ui| {
            let (top, bot) = split(ui, Icon::Measure, "Messen", true, self.measure);
            if top.clicked() { self.toggle_measure(); }
            drop_menu(ui, "measure", &bot, |ui| {
                for n in ["Zusammenfassung", "Länge", "Abstand", "Winkel", "Durchmesser", "Fläche", "Volumen"] {
                    if menu_item(ui, Icon::Measure, n, true) && !self.measure { self.toggle_measure(); }
                }
            });
        });
        group(ui, "Abfrage", |ui| { large(ui, Icon::Probe, "Simulations-\nTaster", false, false); });
        let ((), t) = group_drop(ui, "Geometrie prüfen", |ui| {
            stack(ui, |ui| {
                small_drop(ui, Icon::GeomReport, "Geometriebericht", false, false);
                small(ui, Icon::DraftCheck, "Schräge", false, false);
                small(ui, Icon::Mold, "Konstruktionsrichtung", false, false);
            });
            stack(ui, |ui| {
                let (m, _) = small_drop(ui, Icon::PairClear, "Paarabstand", true, false);
                if m.on_hover_text("Abstand zwischen zwei Elementen messen").clicked() && !self.measure { self.toggle_measure(); }
                small(ui, Icon::MeshSurf, "Netzfläche", false, false);
                small(ui, Icon::DevAngle, "Abweichwinkel", false, false);
            });
            stack(ui, |ui| { small_drop(ui, Icon::Curvature, "Krümmung", false, false); });
        });
        group_menu(ui, "inspect", &t, |ui| { menu_item(ui, Icon::Curvature, "Abschnitt", false); });
        group(ui, "Konstruktionsstudie", |ui| {
            large(ui, Icon::DimBound, "Bemaßungsberandungen", false, false);
            stack(ui, |ui| {
                tiny(ui, Icon::FamilyTable, "Sensitivitätsanalyse", false, false, false);
                tiny(ui, Icon::ZoomSel, "Machbarkeit", false, false, true);
                tiny(ui, Icon::DevAngle, "Optimierung", false, false, false);
            });
            large(ui, Icon::Simulate, "Simulate\nAnalyse", false, false);
        });
        group(ui, "Sicherheit", |ui| { large(ui, Icon::FieldOfView, "Gesichtsfeld", false, false); });
    }

    pub fn toggle_measure(&mut self) {
        self.measure = !self.measure;
        self.selection.clear();
        if self.measure { self.say("Messen: ein oder zwei Elemente wählen.", MsgKind::Prompt); }
    }

    /// Remembers the camera so Ansicht ▸ Zurück can restore it.
    pub fn push_view(&mut self) { self.creo.prev_view = Some(self.camera.clone()); }

    fn view_ribbon(&mut self, ui: &mut egui::Ui) {
        let sel = !self.tree_selection.is_empty();
        group(ui, "Sichtbarkeit", |ui| {
            stack(ui, |ui| {
                tiny(ui, Icon::Layers, "Ebenen", false, false, false);
                if tiny(ui, Icon::Hide, "Ausblenden", sel, false, false).clicked() { let ids = self.tree_selection.clone(); self.set_hidden(&ids, true); }
                let r = tiny(ui, Icon::Show, "Einblenden", true, false, true);
                drop_menu(ui, "show", &r, |ui| {
                    if menu_item(ui, Icon::Show, "Einblenden", sel) { let ids = self.tree_selection.clone(); self.set_hidden(&ids, false); }
                    if menu_item(ui, Icon::Show, "Alle einblenden", true) {
                        let ids: Vec<u64> = self.data.features.iter().filter(|f| f.hidden).map(|f| f.id).collect();
                        if !ids.is_empty() { self.set_hidden(&ids, false); }
                    }
                });
            });
            stack(ui, |ui| { tiny(ui, Icon::Suppress, "Status", false, false, true); });
        });
        let ((), t) = group_drop(ui, "Farbeffekt", |ui| {
            stack(ui, |ui| {
                let r = tiny(ui, Icon::Appearance, "Darstellungen", true, self.windows.appearance, true);
                if r.clicked() { self.windows.appearance = !self.windows.appearance; }
                tiny(ui, Icon::Scene, "Szenen", false, false, true);
            });
        });
        group_menu(ui, "appearance", &t, |ui| { if menu_item(ui, Icon::Appearance, "Darstellungs-Manager", true) { self.windows.appearance = true; } });
        let ((), t) = group_drop(ui, "Orientierung", |ui| {
            if large(ui, Icon::Refit, "Neu\neinpassen", true, false).clicked() { self.push_view(); self.fit_pending = true; }
            if large(ui, Icon::ZoomSel, "Auf Auswahl\nzoomen", !self.selection.is_empty(), false).clicked() { let sel = self.selection.clone(); self.zoom_to(&sel); }
            if large(ui, Icon::ZoomIn, "Vergrößern", true, false).clicked() { self.push_view(); self.camera.scale /= 1.25; }
            stack(ui, |ui| {
                tiny(ui, Icon::Pan, "Schwenken (mittlere Maustaste + Umschalt)", false, false, false);
                if tiny(ui, Icon::ZoomOut, "Verkleinern", true, false, false).clicked() { self.push_view(); self.camera.scale *= 1.25; }
                if tiny(ui, Icon::Repaint, "Neu zeichnen", true, false, false).clicked() { self.buffers = Arc::new(render::ModelBuffers { generation: 0, solid: vec![], edges: vec![] }); }
            });
            let (top, bot) = split(ui, Icon::SavedViews, "Gespeicherte\nOrientierungen", true, false);
            let id = ui.id().with("saved_views");
            if top.clicked() || bot.clicked() { ui.memory_mut(|m| m.toggle_popup(id)); }
            popup_menu(ui, id, &bot, |ui| { if self.views_menu(ui) { self.normal_view(); } });
            if large(ui, Icon::Csys, "Standardorientierung", true, false).clicked() { self.push_view(); self.camera.rot = StdView::Standard.rotation(); self.fit_pending = true; }
            if large(ui, Icon::ViewBack, "Zurück", self.creo.prev_view.is_some(), false).clicked() {
                if let Some(c) = self.creo.prev_view.take() { let cur = self.camera.clone(); self.camera = c; self.creo.prev_view = Some(cur); }
            }
        });
        group_menu(ui, "orient", &t, |ui| {
            if menu_item(ui, Icon::Perspective, "Perspektivansicht", true) { self.camera.perspective = !self.camera.perspective; }
            if menu_item(ui, Icon::SpinCenter, "Drehmittelpunkt", true) { self.show_spin = !self.show_spin; }
        });
        let ((), t) = group_drop(ui, "Modelldarstellung", |ui| {
            stack(ui, |ui| {
                let r = tiny(ui, style_icon(self.style), "Anzeigestil", true, false, true);
                drop_menu(ui, "style", &r, |ui| self.style_menu(ui));
                if tiny(ui, Icon::Section, "Schnitt", true, self.section.is_some(), true).clicked() { self.toggle_section(); }
                if tiny(ui, Icon::SavedViews, "Ansichtsmanager", true, false, true).clicked() { self.windows.controls = false; }
            });
            stack(ui, |ui| {
                if tiny(ui, Icon::Transparent, "Transparent", true, self.transparent, false).clicked() { self.transparent = !self.transparent; }
                if tiny(ui, Icon::Perspective, "Perspektive", true, self.camera.perspective, false).clicked() { self.camera.perspective = !self.camera.perspective; }
            });
        });
        group_menu(ui, "display", &t, |ui| self.style_menu(ui));
        if let Some(mut s) = self.section {
            group(ui, "Schnitt", |ui| {
                stack(ui, |ui| {
                    let planes: Vec<(u64, String)> = self.result.planes.iter().map(|p| (p.id, p.name.clone())).collect();
                    let span = self.result.topo.bbox.diagonal().max(100.0);
                    egui::ComboBox::from_id_salt("secplane").width(110.0).selected_text(planes.iter().find(|p| p.0 == s.plane).map(|p| p.1.clone()).unwrap_or_default()).show_ui(ui, |ui| {
                        for (id, n) in &planes { ui.selectable_value(&mut s.plane, *id, n); }
                    });
                    ui.add(egui::Slider::new(&mut s.offset, -span..=span).text("Versatz"));
                    ui.checkbox(&mut s.flip, "Seite umkehren");
                });
            });
            self.section = Some(s);
        }
        if self.in_asm() { self.asm_view_group(ui); }
        let ((), t) = group_drop(ui, "Anzeigen", |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    if tiny(ui, Icon::Plane, "Ebenenanzeige", true, self.show_planes, false).clicked() { self.show_planes = !self.show_planes; }
                    if tiny(ui, Icon::Axis, "Achsenanzeige", true, self.show_axes, false).clicked() { self.show_axes = !self.show_axes; }
                    if tiny(ui, Icon::Point, "Punktanzeige", true, self.show_points, false).clicked() { self.show_points = !self.show_points; }
                    if tiny(ui, Icon::Csys, "KoordSys-Anzeige", true, self.show_csys, false).clicked() { self.show_csys = !self.show_csys; }
                    if tiny(ui, Icon::SpinCenter, "Drehmittelpunkt", true, self.show_spin, false).clicked() { self.show_spin = !self.show_spin; }
                    if tiny(ui, Icon::DatumDisplay, "Bezugsbeschriftungen", true, self.show_tags, false).clicked() { self.show_tags = !self.show_tags; }
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    if tiny(ui, Icon::PlaneTags, "Ebenenbeschriftungen", true, self.show_tags, false).clicked() { self.show_tags = !self.show_tags; }
                    if tiny(ui, Icon::Sketch, "Kurvenanzeige", true, self.show_curves, false).clicked() { self.show_curves = !self.show_curves; }
                    tiny(ui, Icon::Note3d, "Anmerkungsanzeige", false, false, false);
                    tiny(ui, Icon::DimDisplay, "Bemaßungsanzeige", false, false, false);
                    tiny(ui, Icon::DatumTarget, "Bezugszielanzeige", false, false, false);
                    tiny(ui, Icon::GeomTol, "Toleranzanzeige", false, false, true);
                });
            });
        });
        group_menu(ui, "showdisp", &t, |ui| {
            check_list(ui, &mut [(Icon::Plane, "Ebenenanzeige", &mut self.show_planes), (Icon::Axis, "Achsenanzeige", &mut self.show_axes), (Icon::Point, "Punktanzeige", &mut self.show_points),
                (Icon::Csys, "KoordSys-Anzeige", &mut self.show_csys), (Icon::CurvePoints, "Kurvenanzeige", &mut self.show_curves), (Icon::PlaneTags, "Bezugsbeschriftungen", &mut self.show_tags)]);
        });
        let ((), t) = group_drop(ui, "Fenster", |ui| {
            stack(ui, |ui| {
                tiny(ui, Icon::ActivateWin, "Fenster aktivieren", false, false, false);
                if tiny(ui, Icon::Close, "Schließen", true, false, false).clicked() { self.close_model(); }
            });
            stack(ui, |ui| { tiny(ui, Icon::Windows, "Fenster", false, false, true); });
        });
        group_menu(ui, "windows", &t, |ui| { if menu_item(ui, Icon::FullScreen, "Vollbild", true) { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(true)); } });
    }

    fn tools_ribbon(&mut self, ui: &mut egui::Ui) {
        let free = self.editor.is_none() && self.sketch.is_none();
        let ((), t) = group_drop(ui, "Untersuchen", |ui| {
            large(ui, Icon::Bom, "Stückliste", false, false).on_hover_text("Nur in Baugruppen");
            if large(ui, Icon::Player, "Modell-\nPlayer", free, self.creo.player).on_hover_text("Die KE-Historie abspielen").clicked() { self.creo.player = !self.creo.player; }
            stack(ui, |ui| {
                if tiny(ui, Icon::ModelInfo, "Modellinfo", true, self.windows.info, false).clicked() { self.windows.info = !self.windows.info; }
                if tiny(ui, Icon::EditDef, "KE-Info", !self.tree_selection.is_empty(), false, false).clicked() { self.creo.ref_viewer = true; }
                if tiny(ui, Icon::Recent, "Meldungsprotokoll", true, self.windows.messages, false).clicked() { self.windows.messages = !self.windows.messages; }
            });
            if large(ui, Icon::RefViewer, "Referenz-\nViewer", true, self.creo.ref_viewer).on_hover_text("Eltern und Kinder des gewählten KE anzeigen").clicked() { self.creo.ref_viewer = !self.creo.ref_viewer; }
            if large(ui, Icon::Search, "Suchen", true, self.creo.search.is_some()).on_hover_text("KEs nach Namen suchen und auswählen").clicked() { self.creo.search = if self.creo.search.is_some() { None } else { Some(String::new()) }; }
            stack(ui, |ui| {
                tiny(ui, Icon::Copy, "Kopieren", false, false, false);
                tiny(ui, Icon::DevAngle, "Abstand", false, false, false);
                tiny(ui, Icon::Settings, "Konfiguration", false, false, true);
            });
        });
        group_menu(ui, "investigate", &t, |ui| { if menu_item(ui, Icon::ModelInfo, "Modellinfo", true) { self.windows.info = true; } });
        let ((), t) = group_drop(ui, "Modellabsicht", |ui| {
            if large(ui, Icon::CompInterface, "Komponentenschnittstelle", !self.in_asm(), self.creo.iface_dialog).on_hover_text("Platzierungsreferenzen festlegen, die das Teil in Baugruppen mitbringt").clicked() { self.creo.iface_dialog = !self.creo.iface_dialog; }
            large(ui, Icon::PublishGeom, "Publiziergeometrie", false, false);
            if large(ui, Icon::FamilyTable, "Familientabelle", true, self.windows.family).on_hover_text("Instanzen dieses Teils in einer Tabelle definieren").clicked() { self.windows.family = !self.windows.family; }
            stack(ui, |ui| {
                if small(ui, Icon::Parameters, "Parameter", true, self.windows.parameters).clicked() { self.windows.parameters = !self.windows.parameters; }
                small(ui, Icon::SwitchDims, "Bemaßungen wechseln", false, false);
                if small(ui, Icon::Relations, "Beziehungen", true, self.windows.relations).clicked() { self.windows.relations = !self.windows.relations; }
            });
        });
        group_menu(ui, "intent2", &t, |ui| { if menu_item(ui, Icon::Parameters, "Parameter", true) { self.windows.parameters = true; } });
        group(ui, "Dienstprogramme", |ui| {
            stack(ui, |ui| {
                small(ui, Icon::UdfLib, "UDF-Bibliothek", false, false);
                if small(ui, Icon::Appearance, "Farbeffekte-Manager", true, self.windows.appearance).clicked() { self.windows.appearance = !self.windows.appearance; }
                small(ui, Icon::AuxApps, "Hilfsanwendungen", false, false);
            });
            stack(ui, |ui| {
                small(ui, Icon::ImageEditor, "Bildeditor", false, false);
                small(ui, Icon::Hatch, "Schraffur-Designer", false, false);
            });
        });
        group(ui, "Erweiterte Realität", |ui| {
            split(ui, Icon::ArTarget, "Ziel", false, false);
            stack(ui, |ui| {
                tiny(ui, Icon::Globe, "Hochladen", false, false, false);
                tiny(ui, Icon::Globe, "Teilen", false, false, false);
            });
        });
    }

    fn sketch_close(&mut self, ui: &mut egui::Ui) -> Outcome {
        let mut out = Outcome::None;
        let Some(s) = self.sketch.as_ref() else { return out };
        let sk = s.sketch.clone();
        ui.add_space(4.0);
        group(ui, "Schließen", |ui| {
            if large(ui, Icon::Ok, "OK", true, false).on_hover_text("Skizze übernehmen").clicked() { out = Outcome::Done(sk.clone()); }
            if large(ui, Icon::Cancel, "Abbrechen", true, false).on_hover_text("Skizzenänderungen verwerfen").clicked() { out = Outcome::Cancel; }
        });
        out
    }

    fn sketch_ribbon(&mut self, ui: &mut egui::Ui) {
        // Creo 10's compact Skizze tab: icon grids with ▾ variant menus, and group menus
        let mut sketch_view = false;
        let Some(s) = self.sketch.as_mut() else { return };
        let tool = s.tool;
        let mut set: Option<Tool> = None;
        // icon with ▾: the icon starts the last used variant, the arrow lists all variants
        let variants = |ui: &mut egui::Ui, key: &str, current: Tool, opts: &[(Tool, Icon, &str, &str)], set: &mut Option<Tool>| {
            let icon = opts.iter().find(|o| o.0 == current).map(|o| o.1).unwrap_or(opts[0].1);
            let tip = opts.iter().find(|o| o.0 == current).map(|o| o.2).unwrap_or(opts[0].2);
            let r = tiny(ui, icon, tip, true, opts.iter().any(|o| o.0 == tool), true);
            let id = ui.id().with(key);
            let on_arrow = r.interact_pointer_pos().is_some_and(|p| p.x > r.rect.right() - 12.0);
            if r.clicked() { if on_arrow { ui.memory_mut(|m| m.toggle_popup(id)); } else { *set = Some(current); } }
            demo_popup(ui, id, key);
            egui::popup_below_widget(ui, id, &r, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (t, ic, n, sc) in opts { if menu_row(ui, Some(*ic), n, sc, true, tool == *t).clicked() { *set = Some(*t); } }
            });
        };
        let (mut setup, mut import) = (false, false);
        let (_, title) = group_drop(ui, "Setup", |ui| {
            stack(ui, |ui| {
                if tiny(ui, Icon::SketchSetup, "Skizzen-Setup: Skizzierebene und Orientierung festlegen", true, false, false).clicked() { setup = true; }
                if tiny(ui, Icon::References, "Referenzen", true, s.refs_open, false).clicked() { s.refs_open = !s.refs_open; }
                if tiny(ui, Icon::SketchView, "Skizzenansicht: Skizzierebene parallel zum Bildschirm ausrichten", true, false, false).clicked() { sketch_view = true; }
            });
        });
        group_menu(ui, "sk_setup", &title, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if menu_row(ui, Some(Icon::SketchSetup), "Skizzen-Setup", "", true, false).clicked() { setup = true; }
            if menu_row(ui, Some(Icon::References), "Referenzen", "", true, false).clicked() { s.refs_open = true; }
            if menu_row(ui, Some(Icon::SketchView), "Skizzenansicht", "", true, false).clicked() { sketch_view = true; }
        });
        group(ui, "Einfügen", |ui| {
            if large(ui, Icon::SkImport, "Importieren", true, false).on_hover_text("Geometrie aus einer Datei (DXF, IGES, AI) in die Skizze importieren").clicked() { import = true; }
            let pr = large(ui, Icon::SkPalette, "Palette", true, s.palette_open || tool == Tool::Palette).on_hover_text("Skizzierer-Palette: vordefinierte Formen (Polygone, Profile, Formen, Sterne) in die Skizze einfügen");
            if pr.clicked() { s.palette_open = !s.palette_open; }
        });
        let (has_ent, has_dim, has_weak) = (s.has_selected(false), s.has_selected(true), s.has_weak_selected());
        let (_, title) = group_drop(ui, "Operationen", |ui| {
            stack(ui, |ui| {
                if tiny(ui, Icon::Cut, "Ausschneiden (Strg+X)", has_ent, false, false).clicked() { s.copy_selection(true); }
                if tiny(ui, Icon::Copy, "Kopieren (Strg+C)", has_ent, false, false).clicked() { s.copy_selection(false); }
                if tiny(ui, Icon::Paste, "Einfügen (Strg+V)", s.can_paste(), false, false).clicked() { s.paste(None); }
            });
            let (top, bot) = split(ui, Icon::SkSelect, "Auswählen", true, tool == Tool::Select);
            if top.clicked() { set = Some(Tool::Select); s.chain_select = false; }
            let id = ui.id().with("sk_select");
            if bot.clicked() { ui.memory_mut(|m| m.toggle_popup(id)); }
            demo_popup(ui, id, "sk_select");
            egui::popup_below_widget(ui, id, &bot, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if menu_row(ui, Some(Icon::SkSelect), "Einzeln", "", true, tool == Tool::Select && !s.chain_select).clicked() { set = Some(Tool::Select); s.chain_select = false; }
                if menu_row(ui, Some(Icon::SelChain), "Kette", "", true, tool == Tool::Select && s.chain_select).clicked() { set = Some(Tool::Select); s.chain_select = true; }
                if menu_row(ui, None, "Alle Geometrie", "", true, false).clicked() { s.select_all(false); }
                if menu_row(ui, Some(Icon::SelAll), "Alle", "Strg+Alt+A", true, false).clicked() { s.select_all(true); }
            });
        });
        group_menu(ui, "sk_ops", &title, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if menu_row(ui, None, "Löschen", "Entf", !s.selection.is_empty(), false).clicked() { s.delete_selection(); }
            if menu_row(ui, Some(Icon::SkReplace), "Ersetzen", "", has_ent, false).clicked() { s.start_replace(); }
            if menu_row(ui, Some(Icon::SkConstruction), "Konstruktion", "Umschalt+G", has_ent, false).clicked() { s.toggle_construction(); }
            if menu_row(ui, Some(Icon::SkLock), "Sperre umschalten", "", has_dim, false).clicked() { s.toggle_lock(); }
            // Konvertieren in ▸ opens to the right while the pointer is on the row or the submenu
            let r = menu_row(ui, None, "Konvertieren in", "▸", has_dim, false);
            let sub_id = ui.id().with("sk_convert");
            let over_sub = ui.ctx().data(|d| d.get_temp::<bool>(sub_id)).unwrap_or(false);
            if has_dim && (r.hovered() || over_sub || std::env::var("FREO_DEMO_POPUP").as_deref() == Ok("sk_ops")) {
                let area = egui::Area::new(sub_id).fixed_pos(r.rect.right_top()).order(egui::Order::Tooltip).show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).inner_margin(2.0).show(ui, |ui| {
                        if menu_row(ui, Some(Icon::SkConvert), "Stark", "", has_weak, false).clicked() { s.strengthen_selection(); }
                        if menu_row(ui, Some(Icon::RefDim), "Referenz", "", true, false).clicked() { s.convert_to_reference(); }
                    });
                });
                let inside = ui.ctx().pointer_hover_pos().is_some_and(|p| area.response.rect.expand(4.0).contains(p));
                ui.ctx().data_mut(|d| d.insert_temp(sub_id, inside));
            }
        });
        group(ui, "Bezug", |ui| {
            stack(ui, |ui| {
                if tiny(ui, Icon::SkCenterline, "Mittellinie", true, tool == Tool::Centerline, false).clicked() { set = Some(Tool::Centerline); }
                if tiny(ui, Icon::SkPoint, "Punkt", true, tool == Tool::Point, false).clicked() { set = Some(Tool::Point); }
            });
        });
        let construction = s.construction;
        group(ui, "Skizze", |ui| {
            if large(ui, Icon::SkConstruction, "Konstruktionsmodus", true, construction).on_hover_text("Neue Elemente als Konstruktionsgeometrie skizzieren").clicked() { s.construction = !s.construction; }
            stack(ui, |ui| {
                variants(ui, "linev", s.last_line, &[(Tool::Line, Icon::SkLine, "Linienkette", "L"), (Tool::LineTangent, Icon::SkLineTangent, "Linientangentiale", "")], &mut set);
                variants(ui, "rectv", s.last_rect, &[(Tool::Rect, Icon::SkRect, "Ecken-Rechteck", "R"), (Tool::SlantRect, Icon::SkSlantRect, "Geneigtes Rechteck", ""), (Tool::CenterRect, Icon::SkCenterRect, "Mittleres Rechteck", ""), (Tool::Parallelogram, Icon::SkParallelogram, "Parallelogramm", "")], &mut set);
                variants(ui, "circv", s.last_circle, &[(Tool::Circle, Icon::SkCircle, "Mitte und Punkt", ""), (Tool::CircleConc, Icon::SkCircleConc, "Konzentrisch", ""), (Tool::Circle3, Icon::SkCircle3, "3 Punkte", ""), (Tool::Circle3T, Icon::SkCircle3T, "3 Tangenten", "")], &mut set);
            });
            stack(ui, |ui| {
                variants(ui, "arcv", s.last_arc, &[(Tool::Arc3, Icon::SkArc3, "3 Punkte/Tangentenende", ""), (Tool::ArcCenter, Icon::SkArcCenter, "Mitte und Endpunkte", ""), (Tool::Arc3T, Icon::SkArc3T, "3 Tangenten", ""), (Tool::ArcConc, Icon::SkArcConc, "Konzentrisch", ""), (Tool::Conic, Icon::SkConic, "Konisch", "")], &mut set);
                variants(ui, "ellv", s.last_ellipse, &[(Tool::EllipseAxis, Icon::SkEllipseAxis, "Achsenendenellipse", ""), (Tool::Ellipse, Icon::SkEllipse, "Mittelpunkt und Achse der Ellipse", "")], &mut set);
                if tiny(ui, Icon::SkSpline, "Spline", true, tool == Tool::Spline, false).clicked() { set = Some(Tool::Spline); }
            });
            stack(ui, |ui| {
                variants(ui, "filv", s.last_fillet, &[(Tool::FilletCirc, Icon::SkFilletTrim, "Kreisförmig", ""), (Tool::Fillet, Icon::SkFillet, "Rund trimmen", ""), (Tool::FilletEllip, Icon::SkFilletEllip, "Elliptisch", ""), (Tool::FilletEllipTrim, Icon::SkFilletEllip, "Elliptisch trimmen", "")], &mut set);
                variants(ui, "chav", s.last_chamfer, &[(Tool::ChamferKeep, Icon::SkChamferTrim, "Fase", ""), (Tool::Chamfer, Icon::SkChamfer, "Fasentrimmung", "")], &mut set);
                if tiny(ui, Icon::SkText, "Text", true, tool == Tool::Text, false).clicked() { set = Some(Tool::Text); }
            });
            stack(ui, |ui| {
                if tiny(ui, Icon::SkOffset, "Versatz: eine Kante oder ein Element versetzt übernehmen", true, tool == Tool::Offset, false).clicked() { set = Some(Tool::Offset); }
                if tiny(ui, Icon::SkThicken, "Aufdicken: eine Kante oder ein Element mit Dicke übernehmen", true, tool == Tool::Thicken, false).clicked() { set = Some(Tool::Thicken); }
                if tiny(ui, Icon::SkProject, "Projizieren: Modellkanten als Skizzenelemente übernehmen", true, tool == Tool::Project, false).clicked() { set = Some(Tool::Project); }
            });
            stack(ui, |ui| {
                variants(ui, "clv", s.last_centerline, &[(Tool::Centerline, Icon::SkCenterline, "Mittellinie", ""), (Tool::CenterlineTangent, Icon::SkCenterlineTangent, "Mittellinientangentiale", "")], &mut set);
                if tiny(ui, Icon::SkPoint, "Punkt", true, tool == Tool::Point, false).clicked() { set = Some(Tool::Point); }
                if tiny(ui, Icon::SkCsys, "Koordinatensystem", true, tool == Tool::Csys, false).clicked() { set = Some(Tool::Csys); }
            });
        });
        group(ui, "Editieren", |ui| {
            stack(ui, |ui| {
                if tiny(ui, Icon::SkModify, "Ändern", true, s.modify_open, false).clicked() { s.modify_open = !s.modify_open; }
                if tiny(ui, Icon::SkMirror, "Spiegeln", true, tool == Tool::Mirror, false).clicked() { set = Some(Tool::Mirror); }
                if tiny(ui, Icon::SkDivide, "Teilen", true, tool == Tool::Divide, false).clicked() { set = Some(Tool::Divide); }
            });
            stack(ui, |ui| {
                if tiny(ui, Icon::SkDeleteSeg, "Segment löschen", true, tool == Tool::Trim, false).clicked() { set = Some(Tool::Trim); }
                if tiny(ui, Icon::SkCorner, "Ecke", true, tool == Tool::Corner, false).clicked() { set = Some(Tool::Corner); }
                if tiny(ui, Icon::RotResize, "Drehen-Größe ändern", has_ent, s.rot_resize.is_some(), false).clicked() { s.open_rot_resize(); }
            });
        });
        let cons = [
            [(SkCType::Vertical, Icon::ConV, "Vertikal"), (SkCType::Horizontal, Icon::ConH, "Horizontal"), (SkCType::Perpendicular, Icon::ConPerp, "Senkrecht")],
            [(SkCType::Tangent, Icon::ConTan, "Tangential"), (SkCType::Midpoint, Icon::ConMid, "Mittelpunkt"), (SkCType::Coincident, Icon::ConCoinc, "Zusammenfallend")],
            [(SkCType::Symmetric, Icon::ConSym, "Symmetrisch"), (SkCType::Equal, Icon::ConEqual, "Gleich"), (SkCType::Parallel, Icon::ConPar, "Parallel")],
        ];
        let (_, title) = group_drop(ui, "Bedingung definieren", |ui| {
            for col in cons {
                stack(ui, |ui| {
                    for (c, ic, n) in col { if tiny(ui, ic, n, true, tool == Tool::Con(c), false).clicked() { set = Some(Tool::Con(c)); } }
                });
            }
        });
        group_menu(ui, "sk_cons", &title, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (c, ic, n) in cons.iter().flatten() { if menu_row(ui, Some(*ic), n, "", true, tool == Tool::Con(*c)).clicked() { set = Some(Tool::Con(*c)); } }
        });
        group(ui, "Bemaßung", |ui| {
            if large(ui, Icon::SkDimension, "Bemaßung", true, tool == Tool::Dimension).clicked() { set = Some(Tool::Dimension); }
            stack(ui, |ui| {
                if tiny(ui, Icon::Perimeter, "Umfang: Umfangsbemaßung einer Kette mit variabler Bemaßung", true, tool == Tool::Perimeter, false).clicked() { set = Some(Tool::Perimeter); }
                if tiny(ui, Icon::Baseline, "Basislinie: Basis für Ordinatenbemaßungen", true, tool == Tool::Baseline, false).clicked() { set = Some(Tool::Baseline); }
                if tiny(ui, Icon::RefDim, "Referenz: Referenzbemaßung erzeugen", true, tool == Tool::RefDimension, false).clicked() { set = Some(Tool::RefDimension); }
            });
        });
        let (_, title) = group_drop(ui, "Prüfen", |ui| {
            if large(ui, Icon::Requirements, "KE-\nAnforderungen", true, s.requirements_open).on_hover_text("Prüfen, ob die Skizze die Anforderungen des KEs erfüllt").clicked() { s.requirements_open = !s.requirements_open; }
        });
        group_menu(ui, "sk_check", &title, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (ic, n, v) in [(Icon::HlOverlap, "Überlappende Geometrie hervorheben", &mut s.hl_overlap), (Icon::HlIntersect, "Sich schneidende Geometrie hervorheben", &mut s.hl_intersect), (Icon::HlBranch, "Geometrieabzweigungen hervorheben", &mut s.hl_branch), (Icon::OpenEnds, "Offene Enden hervorheben", &mut s.open_ends), (Icon::ShadeLoops, "Geschlossene Schleifen schattieren", &mut s.shade)] {
                if menu_row(ui, Some(ic), n, "", true, *v).clicked() { *v = !*v; }
            }
            ui.separator();
            if menu_row(ui, Some(Icon::InfoIntersect), "Schnittpunkt", "", true, tool == Tool::InfoIntersect).clicked() { set = Some(Tool::InfoIntersect); }
            if menu_row(ui, Some(Icon::InfoTangent), "Tangentialpunkt", "", true, tool == Tool::InfoTangent).clicked() { set = Some(Tool::InfoTangent); }
            if menu_row(ui, Some(Icon::InfoEntity), "Element", "", true, tool == Tool::InfoEntity).clicked() { set = Some(Tool::InfoEntity); }
        });
        if let Some(t) = set { s.set_tool(t); }
        if import {
            if let Some(p) = rfd::FileDialog::new().set_title("Importieren")
                .add_filter("Alle unterstützten Dateien", &["dxf", "igs", "iges", "ai"])
                .add_filter("DXF (*.dxf)", &["dxf"]).add_filter("IGES (*.igs, *.iges)", &["igs", "iges"]).add_filter("Adobe Illustrator (*.ai)", &["ai"]).pick_file() {
                match s.import_file(&p) {
                    Ok(n) => self.say(format!("{} Element(e) aus {} importiert.", n, p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()), MsgKind::Success),
                    Err(e) => self.say(format!("Import fehlgeschlagen: {}", e), MsgKind::Error),
                }
            }
        }
        if setup {
            if let Some(p) = self.editor.as_ref().and_then(|e| e.feature.kind.sketch_plane()) { self.resetup = Some((p, false)); }
            self.say("Ebene oder Fläche auswählen, um Skizzierebene zu definieren.", MsgKind::Prompt);
        }
        if sketch_view { self.sketch_view(); }
    }

    pub fn set_hidden(&mut self, ids: &[u64], hidden: bool) {
        let ids = ids.to_vec();
        if self.in_asm() {
            // components and the assembly's own datums
            self.asm_commit("", |a| {
                for c in &mut a.components { if ids.contains(&c.id) { c.hidden = hidden; } }
                for f in &mut a.skeleton.features { if ids.contains(&f.id) { f.hidden = hidden; } }
            });
            return;
        }
        self.commit("", |d| for f in &mut d.features { if ids.contains(&f.id) { f.hidden = hidden; } });
    }
}

pub fn style_icon(s: Style) -> Icon {
    match s {
        Style::Reflect => Icon::DispReflect,
        Style::ShadedEdges => Icon::DispShadedEdges,
        Style::Shaded => Icon::DispShaded,
        Style::HiddenLine => Icon::DispHidden,
        Style::NoHidden => Icon::DispNoHidden,
        Style::Wireframe => Icon::DispWire,
    }
}
