//! Widgets for dialogs in the look of Creo Parametric 10: grey title bar with window
//! controls, a menu bar, flat buttons, combo boxes, tabs, check boxes and sliders.

use super::icons::{self, Icon};
use super::ribbon::{demo_popup, menu_row, menu_sep, menu_spacing, theme::*, tri};
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind};

pub const BG: Color32 = Color32::from_rgb(240, 241, 242);
pub const LINE: Color32 = Color32::from_rgb(196, 200, 206);
pub const PRIMARY: Color32 = Color32::from_rgb(152, 214, 255);
pub const ROW_SEL: Color32 = Color32::from_rgb(176, 226, 255);
pub const F: f32 = 14.0;

pub fn font() -> FontId {
    FontId::proportional(F)
}

pub fn tw(ui: &egui::Ui, t: &str, size: f32) -> f32 {
    ui.fonts(|f| f.layout_no_wrap(t.to_string(), FontId::proportional(size), Color32::BLACK).size().x)
}

pub fn lbl(ui: &mut egui::Ui, t: &str) -> Response {
    ui.label(RichText::new(t).size(F).color(TEXT))
}
pub fn lbl_dim(ui: &mut egui::Ui, t: &str) -> Response {
    ui.label(RichText::new(t).size(F).color(DIM_TEXT))
}
/// Bold text (the UI font has no bold face: drawn twice, half a pixel apart).
pub fn lbl_bold(ui: &mut egui::Ui, t: &str) {
    let g = ui.painter().layout_no_wrap(t.to_string(), font(), TEXT);
    let (r, _) = ui.allocate_exact_size(g.size() + vec2(1.0, 0.0), Sense::hover());
    ui.painter().galley(r.min, g.clone(), TEXT);
    ui.painter().galley(r.min + vec2(0.6, 0.0), g, TEXT);
}

/// A full-width hairline.
pub fn hline(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, LINE);
}

/// Dialog window: grey title bar (optionally with – and ▢), no margin. Returns false when closed.
pub fn cwin(ctx: &egui::Context, id: &str, title: &str, ctl: bool, pos: Pos2, width: f32, body: impl FnOnce(&mut egui::Ui)) -> bool {
    cwin_in(ctx, id, title, ctl, pos, width, egui::Order::Middle, body)
}

/// A dialog opened from another dialog: it stays in front of it, as Creo's modal dialogs do.
pub fn cwin_top(ctx: &egui::Context, id: &str, title: &str, ctl: bool, pos: Pos2, width: f32, body: impl FnOnce(&mut egui::Ui)) -> bool {
    cwin_in(ctx, id, title, ctl, pos, width, egui::Order::Foreground, body)
}

#[allow(clippy::too_many_arguments)]
fn cwin_in(ctx: &egui::Context, id: &str, title: &str, ctl: bool, pos: Pos2, width: f32, order: egui::Order, body: impl FnOnce(&mut egui::Ui)) -> bool {
    let mut open = true;
    // a dialog that opens comes to the front (also in front of the dialog it was opened from)
    let fresh = ctx.memory(|m| m.area_rect(egui::Id::new(id)).is_none());
    let frame = egui::Frame::window(&ctx.style()).fill(BG).inner_margin(0.0).corner_radius(4.0).stroke(Stroke::new(1.0_f32, Color32::from_rgb(150, 156, 164)));
    egui::Window::new(title)
        .id(egui::Id::new(id))
        .order(order)
        .fade_in(false)
        .fade_out(false)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .default_pos(pos)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.visuals_mut().window_fill = BG;
            ui.visuals_mut().override_text_color = Some(TEXT);
            ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
            if chead(ui, title, ctl) {
                open = false;
            }
            body(ui);
        });
    if fresh {
        ctx.move_to_top(egui::LayerId::new(order, egui::Id::new(id)));
    }
    open
}

fn chead(ui: &mut egui::Ui, title: &str, ctl: bool) -> bool {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, Color32::from_rgb(218, 222, 224));
    p.text(r.center(), Align2::CENTER_CENTER, title, font(), TEXT);
    let c = pos2(r.right() - 18.0, r.center().y);
    let resp = ui.interact(Rect::from_center_size(c, vec2(26.0, 24.0)), ui.id().with("cwin_close"), Sense::click());
    let grey = Color32::from_rgb(96, 102, 110);
    let st = Stroke::new(2.0_f32, if resp.hovered() { Color32::from_rgb(214, 48, 40) } else { grey });
    p.line_segment([c + vec2(-4.0, -4.0), c + vec2(4.0, 4.0)], st);
    p.line_segment([c + vec2(-4.0, 4.0), c + vec2(4.0, -4.0)], st);
    if ctl {
        let m = c - vec2(28.0, 0.0);
        p.rect_stroke(Rect::from_center_size(m, vec2(10.0, 8.0)), 0.0, Stroke::new(1.2_f32, grey), StrokeKind::Inside);
        p.line_segment([m + vec2(-5.0, -3.4), m + vec2(5.0, -3.4)], Stroke::new(2.0_f32, grey));
        let n = c - vec2(56.0, 0.0);
        p.line_segment([n + vec2(-4.0, 3.0), n + vec2(4.0, 3.0)], Stroke::new(2.0_f32, grey));
    }
    resp.clicked()
}

/// Padded section of a dialog with normal spacing.
pub fn pad<R>(ui: &mut egui::Ui, m: f32, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::NONE
        .inner_margin(m)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            f(ui)
        })
        .inner
}

/// One entry of a dialog menu.
pub enum M {
    /// label, shortcut, enabled
    I(&'static str, &'static str, bool),
    Ic(Icon, &'static str, &'static str, bool),
    Sep,
    /// label, enabled, entries: a row with ▸ whose entries fold out
    Sub(&'static str, bool, Vec<M>),
    /// label, checked, enabled
    Chk(&'static str, bool, bool),
    /// label, selected
    Radio(&'static str, bool),
}

fn popup_look(ui: &mut egui::Ui) {
    let v = ui.visuals_mut();
    v.window_fill = Color32::WHITE;
    v.menu_corner_radius = egui::CornerRadius::same(2);
    v.window_stroke = Stroke::new(1.0_f32, Color32::from_rgb(170, 176, 184));
    ui.spacing_mut().menu_margin = egui::Margin::same(1);
}

/// Rows of a dialog menu; returns the label that was clicked.
pub fn menu_rows(ui: &mut egui::Ui, items: &[M]) -> Option<&'static str> {
    let mut hit = None;
    menu_spacing(ui);
    for m in items {
        match m {
            M::I(l, k, en) => {
                if menu_row(ui, None, l, k, *en, false).clicked() {
                    hit = Some(*l);
                }
            }
            M::Ic(ic, l, k, en) => {
                if menu_row(ui, Some(*ic), l, k, *en, false).clicked() {
                    hit = Some(*l);
                }
            }
            M::Sep => menu_sep(ui),
            M::Sub(l, en, sub) => {
                if *en && !sub.is_empty() {
                    super::ribbon::flyout(ui, l, |ui| {
                        if let Some(h) = menu_rows(ui, sub) {
                            hit = Some(h);
                        }
                    });
                } else {
                    menu_row(ui, None, l, "▸", false, false);
                }
            }
            M::Chk(l, on, en) => {
                let r = menu_row(ui, None, l, "", *en, false);
                let b = Rect::from_center_size(pos2(r.rect.left() + 18.0, r.rect.center().y), vec2(14.0, 14.0));
                super::creo::check_box(ui.painter(), b, *on);
                if !*en {
                    ui.painter().rect_filled(b, 1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 170));
                }
                if r.clicked() {
                    hit = Some(*l);
                }
            }
            M::Radio(l, on) => {
                let r = menu_row(ui, None, l, "", true, false);
                radio_dot(ui.painter(), pos2(r.rect.left() + 18.0, r.rect.center().y), *on, true);
                if r.clicked() {
                    hit = Some(*l);
                }
            }
        }
    }
    hit
}

/// Menu bar of a dialog. `dlg` names the dialog (FREO_DEMO_POPUP=menu:<dlg>:<Menu> keeps a menu open).
pub fn menubar(ui: &mut egui::Ui, dlg: &str, menus: &[(&'static str, Vec<M>)]) -> Option<&'static str> {
    let mut hit = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(2.0);
        for (name, items) in menus {
            let w = tw(ui, name, 14.5) + 24.0;
            let (r, resp) = ui.allocate_exact_size(vec2(w, 34.0), Sense::click());
            let id = ui.id().with(("cmenu", dlg, *name));
            if resp.clicked() {
                ui.memory_mut(|m| m.toggle_popup(id));
            }
            demo_popup(ui, id, &format!("menu:{}:{}", dlg, name));
            let open = ui.memory(|m| m.is_popup_open(id));
            if open {
                ui.painter().rect_filled(r, 0.0, Color32::from_rgb(135, 206, 250));
            } else if resp.hovered() {
                ui.painter().rect_filled(r, 0.0, HOVER_BG);
            }
            ui.painter().text(r.center(), Align2::CENTER_CENTER, *name, FontId::proportional(14.5), TEXT);
            ui.scope(|ui| {
                popup_look(ui);
                egui::popup_below_widget(ui, id, &resp, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                    ui.set_min_width(170.0);
                    if let Some(h) = menu_rows(ui, items) {
                        hit = Some(h);
                    }
                });
            });
        }
    });
    hline(ui);
    hit
}

/// A ▾ button's menu (Optionen ▾, Datei ▾).
pub fn drop_rows(ui: &mut egui::Ui, key: &str, resp: &Response, items: &[M]) -> Option<&'static str> {
    let id = ui.id().with(("cdrop", key));
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    demo_popup(ui, id, &format!("drop:{}", key));
    let mut hit = None;
    ui.scope(|ui| {
        popup_look(ui);
        egui::popup_below_widget(ui, id, resp, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(170.0);
            hit = menu_rows(ui, items);
        });
    });
    hit
}

/// Creo's flat push button. `kind`: 0 normal, 1 default (light blue), 2 not available.
pub fn btn(ui: &mut egui::Ui, label: &str, w: f32, kind: u8) -> bool {
    let w = w.max(tw(ui, label, F) + 24.0);
    let (r, resp) = ui.allocate_exact_size(vec2(w, 32.0), if kind == 2 { Sense::hover() } else { Sense::click() });
    let p = ui.painter();
    let fill = match kind {
        2 => Color32::from_rgb(234, 236, 238),
        1 => {
            if resp.hovered() {
                Color32::from_rgb(130, 204, 250)
            } else {
                PRIMARY
            }
        }
        _ => {
            if resp.hovered() {
                HOVER_BG
            } else {
                Color32::from_rgb(250, 250, 250)
            }
        }
    };
    p.rect_filled(r, 1.0, fill);
    p.rect_stroke(r, 1.0, Stroke::new(1.0_f32, if kind == 1 { Color32::from_rgb(120, 190, 235) } else { LINE }), StrokeKind::Inside);
    p.text(r.center(), Align2::CENTER_CENTER, label, font(), if kind == 2 { DIM_TEXT } else { TEXT });
    resp.clicked()
}

/// Square bordered button that shows only an icon.
pub fn ibox(ui: &mut egui::Ui, icon: Icon, enabled: bool, tip: &str) -> Response {
    let (r, resp) = ui.allocate_exact_size(vec2(36.0, 32.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    p.rect_filled(
        r,
        1.0,
        if !enabled {
            Color32::from_rgb(234, 236, 238)
        } else if resp.hovered() {
            HOVER_BG
        } else {
            Color32::from_rgb(250, 250, 250)
        },
    );
    p.rect_stroke(r, 1.0, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
    icons::paint(p, Rect::from_center_size(r.center(), vec2(18.0, 18.0)), icon, enabled);
    if tip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tip)
    }
}

/// Flat toolbar icon (no border until hovered).
pub fn tool(ui: &mut egui::Ui, icon: Icon, enabled: bool, tip: &str) -> Response {
    let (r, resp) = ui.allocate_exact_size(vec2(32.0, 30.0), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && resp.hovered() {
        ui.painter().rect_filled(r, 2.0, HOVER_BG);
        ui.painter().rect_stroke(r, 2.0, Stroke::new(1.0_f32, HOVER_BORDER), StrokeKind::Inside);
    }
    icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(20.0, 20.0)), icon, enabled);
    if tip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tip)
    }
}

/// Flat toolbar button that shows a short text glyph (=?, ƒx).
pub fn tool_text(ui: &mut egui::Ui, text: &str, tip: &str) -> Response {
    let (r, resp) = ui.allocate_exact_size(vec2(32.0, 30.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, 2.0, HOVER_BG);
        ui.painter().rect_stroke(r, 2.0, Stroke::new(1.0_f32, HOVER_BORDER), StrokeKind::Inside);
    }
    ui.painter().text(r.center(), Align2::CENTER_CENTER, text, FontId::proportional(15.0), Color32::from_rgb(60, 66, 74));
    resp.on_hover_text(tip)
}

/// Combo box. Returns true when another entry was chosen.
pub fn combo(ui: &mut egui::Ui, key: &str, w: f32, cur: &mut usize, items: &[&str], enabled: bool, icon: Option<Icon>) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(w, 32.0), if enabled { Sense::click() } else { Sense::hover() });
    let id = ui.id().with(("ccombo", key));
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    if enabled {
        demo_popup(ui, id, &format!("combo:{}", key));
    }
    let open = ui.memory(|m| m.is_popup_open(id));
    let p = ui.painter();
    p.rect_filled(r, 1.0, if enabled { Color32::from_rgb(250, 250, 250) } else { Color32::from_rgb(240, 241, 242) });
    p.rect_stroke(r, 1.0, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
    let ar = Rect::from_min_max(pos2(r.right() - 22.0, r.top()), r.max);
    if open {
        p.rect_filled(ar.shrink(1.0), 0.0, Color32::from_rgb(200, 204, 210));
    }
    p.line_segment([ar.left_top() + vec2(0.0, 1.0), ar.left_bottom() - vec2(0.0, 1.0)], Stroke::new(1.0_f32, LINE));
    tri(p, ar.center() + vec2(0.0, 1.0), if enabled { TEXT } else { DIM_TEXT });
    let mut x = r.left() + 10.0;
    if let Some(ic) = icon {
        icons::paint(p, Rect::from_center_size(pos2(x + 8.0, r.center().y), vec2(16.0, 16.0)), ic, enabled);
        x += 20.0;
    }
    let text = items.get(*cur).copied().unwrap_or("");
    p.with_clip_rect(Rect::from_min_max(r.min, pos2(ar.left() - 2.0, r.bottom()))).text(
        pos2(x, r.center().y),
        Align2::LEFT_CENTER,
        text,
        font(),
        if enabled { TEXT } else { DIM_TEXT },
    );
    let mut changed = false;
    ui.scope(|ui| {
        popup_look(ui);
        egui::popup_below_widget(ui, id, &resp, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            menu_spacing(ui);
            ui.set_min_width(w - 4.0);
            egui::ScrollArea::vertical().max_height(330.0).show(ui, |ui| {
                for (i, it) in items.iter().enumerate() {
                    let rw = (tw(ui, it, F) + 20.0).max(ui.available_width());
                    let (rr, rs) = ui.allocate_exact_size(vec2(rw, 25.0), Sense::click());
                    if i == *cur {
                        ui.painter().rect_filled(rr, 0.0, ROW_SEL);
                    } else if rs.hovered() {
                        ui.painter().rect_filled(rr, 0.0, HOVER_BG);
                    }
                    let mut tx = rr.left() + 8.0;
                    if let Some(ic) = icon {
                        icons::paint(ui.painter(), Rect::from_center_size(pos2(tx + 8.0, rr.center().y), vec2(16.0, 16.0)), ic, true);
                        tx += 20.0;
                    }
                    ui.painter().text(pos2(tx, rr.center().y), Align2::LEFT_CENTER, *it, font(), TEXT);
                    if rs.clicked() {
                        changed = *cur != i;
                        *cur = i;
                    }
                }
            });
        });
    });
    changed
}

/// Single-line text field in a white box.
pub fn field(ui: &mut egui::Ui, text: &mut String, w: f32, enabled: bool) -> Response {
    ui.add_enabled_ui(enabled, |ui| ui.add_sized(vec2(w, 32.0), egui::TextEdit::singleline(text).font(font()).margin(vec2(8.0, 7.0)))).inner
}

/// A read-only value in a grey box.
pub fn readout(ui: &mut egui::Ui, text: &str, w: f32) {
    let (r, _) = ui.allocate_exact_size(vec2(w, 32.0), Sense::hover());
    ui.painter().rect_filled(r, 1.0, Color32::from_rgb(244, 245, 246));
    ui.painter().rect_stroke(r, 1.0, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
    ui.painter().with_clip_rect(r.shrink(2.0)).text(pos2(r.left() + 10.0, r.center().y), Align2::LEFT_CENTER, text, font(), TEXT);
}

pub fn chk(ui: &mut egui::Ui, on: &mut bool, label: &str, enabled: bool) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(tw(ui, label, F) + 32.0, 26.0), if enabled { Sense::click() } else { Sense::hover() });
    let b = Rect::from_center_size(pos2(r.left() + 10.0, r.center().y), vec2(15.0, 15.0));
    super::creo::check_box(ui.painter(), b, *on);
    if !enabled {
        ui.painter().rect_filled(b, 1.0, Color32::from_rgba_unmultiplied(244, 244, 244, 190));
    }
    ui.painter().text(pos2(r.left() + 28.0, r.center().y), Align2::LEFT_CENTER, label, font(), if enabled { TEXT } else { DIM_TEXT });
    if resp.clicked() {
        *on = !*on;
    }
    resp.clicked()
}

fn radio_dot(p: &egui::Painter, c: Pos2, on: bool, enabled: bool) {
    let col = if enabled { Color32::from_rgb(70, 76, 84) } else { Color32::from_rgb(190, 194, 198) };
    p.circle(c, 7.0, Color32::WHITE, Stroke::new(1.2_f32, col));
    if on {
        p.circle_filled(c, 3.6, col);
    }
}

pub fn radio(ui: &mut egui::Ui, on: bool, label: &str, enabled: bool) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(tw(ui, label, F) + 32.0, 26.0), if enabled { Sense::click() } else { Sense::hover() });
    radio_dot(ui.painter(), pos2(r.left() + 10.0, r.center().y), on, enabled);
    ui.painter().text(pos2(r.left() + 28.0, r.center().y), Align2::LEFT_CENTER, label, font(), if enabled { TEXT } else { DIM_TEXT });
    resp.clicked()
}

/// Tab headers; entries from `dim_from` on are greyed and cannot be chosen.
pub fn tabs(ui: &mut egui::Ui, cur: &mut usize, names: &[&str], dim_from: usize) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (k, t) in names.iter().enumerate() {
            let dim = k >= dim_from;
            let (r, resp) = ui.allocate_exact_size(vec2(tw(ui, t, F) + 28.0, 32.0), if dim { Sense::hover() } else { Sense::click() });
            let sel = *cur == k;
            let p = ui.painter();
            p.rect_filled(r, 0.0, if sel { Color32::from_rgb(250, 250, 250) } else { Color32::from_rgb(236, 238, 240) });
            p.rect_stroke(r, 0.0, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
            if sel {
                p.line_segment([r.left_bottom() + vec2(1.0, 0.0), r.right_bottom() - vec2(1.0, 0.0)], Stroke::new(2.0_f32, Color32::from_rgb(250, 250, 250)));
            }
            p.text(
                r.center(),
                Align2::CENTER_CENTER,
                *t,
                font(),
                if dim {
                    DIM_TEXT
                } else if sel {
                    TEXT
                } else {
                    Color32::from_rgb(110, 114, 120)
                },
            );
            if resp.clicked() {
                *cur = k;
            }
        }
    });
}

/// The white page under tab headers.
pub fn tab_page<R>(ui: &mut egui::Ui, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::NONE
        .fill(Color32::from_rgb(250, 250, 250))
        .stroke(Stroke::new(1.0_f32, LINE))
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.visuals_mut().window_fill = Color32::from_rgb(250, 250, 250);
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            f(ui)
        })
        .inner
}

/// A white list or collector box of a fixed size; returns its rectangle.
pub fn white_box(ui: &mut egui::Ui, w: f32, h: f32, fill: Color32) -> (Rect, Response) {
    let (r, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    ui.painter().rect_filled(r, 0.0, fill);
    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
    (r, resp)
}

/// Slider with Creo's pentagon thumb; `v` runs from 0 to `max`.
pub fn cslider(ui: &mut egui::Ui, v: &mut f32, max: f32, w: f32, enabled: bool) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(w, 28.0), if enabled { Sense::click_and_drag() } else { Sense::hover() });
    let (x0, x1) = (r.left() + 6.0, r.right() - 6.0);
    let mut changed = false;
    if let Some(p) = resp.interact_pointer_pos() {
        let n = ((p.x - x0) / (x1 - x0)).clamp(0.0, 1.0) * max;
        if n != *v {
            *v = n;
            changed = true;
        }
    }
    let p = ui.painter();
    p.line_segment(
        [pos2(x0, r.center().y), pos2(x1, r.center().y)],
        Stroke::new(1.2_f32, if enabled { Color32::from_rgb(130, 136, 144) } else { Color32::from_rgb(200, 203, 207) }),
    );
    let x = x0 + (x1 - x0) * (*v / max).clamp(0.0, 1.0);
    let c = pos2(x, r.center().y);
    let pts = vec![c + vec2(-5.0, -9.0), c + vec2(5.0, -9.0), c + vec2(5.0, 5.0), c + vec2(0.0, 10.0), c + vec2(-5.0, 5.0)];
    p.add(egui::Shape::convex_polygon(
        pts,
        Color32::from_rgb(244, 245, 246),
        Stroke::new(1.0_f32, if enabled { Color32::from_rgb(110, 116, 124) } else { Color32::from_rgb(190, 194, 198) }),
    ));
    changed
}

/// Vertical colour gradient (the graphics window's background).
pub fn gradient(p: &egui::Painter, r: Rect, top: Color32, bot: Color32) {
    let mut m = egui::Mesh::default();
    m.colored_vertex(r.left_top(), top);
    m.colored_vertex(r.right_top(), top);
    m.colored_vertex(r.right_bottom(), bot);
    m.colored_vertex(r.left_bottom(), bot);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(m));
}

/// Shaded ball as in the Farbeffekte-Manager.
pub fn ball(p: &egui::Painter, c: Pos2, r: f32, col: Color32) {
    let mix = |a: Color32, b: Color32, t: f32| {
        Color32::from_rgb(
            (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
            (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
            (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
        )
    };
    p.circle_filled(c, r, mix(col, Color32::BLACK, 0.32));
    for i in 0..18 {
        let t = i as f32 / 17.0;
        p.circle_filled(c + vec2(-r * 0.12 * t, -r * 0.14 * t), r * (0.94 - 0.5 * t), mix(mix(col, Color32::BLACK, 0.2), mix(col, Color32::WHITE, 0.25), t));
    }
    p.circle_filled(c + vec2(-r * 0.22, -r * 0.26), r * 0.14, Color32::from_rgba_unmultiplied(255, 255, 255, 230));
}

/// The ✓ and ✕ buttons that close Creo's feature-like dialogs: (ok, cancel).
pub fn ok_cancel_icons(ui: &mut egui::Ui) -> (bool, bool) {
    let mut out = (false, false);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        for k in [1usize, 0] {
            let (r, resp) = ui.allocate_exact_size(vec2(86.0, 34.0), Sense::click());
            let p = ui.painter();
            p.rect_filled(
                r,
                1.0,
                if k == 0 {
                    Color32::from_rgb(214, 236, 250)
                } else if resp.hovered() {
                    HOVER_BG
                } else {
                    Color32::from_rgb(250, 250, 250)
                },
            );
            p.rect_stroke(r, 1.0, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
            let c = r.center();
            if k == 0 {
                p.add(egui::Shape::line(vec![c + vec2(-8.0, 0.0), c + vec2(-3.0, 5.0), c + vec2(8.0, -6.0)], Stroke::new(3.0_f32, Color32::from_rgb(150, 156, 164))));
            } else {
                let st = Stroke::new(2.6_f32, Color32::from_rgb(50, 54, 60));
                p.line_segment([c + vec2(-5.0, -5.0), c + vec2(5.0, 5.0)], st);
                p.line_segment([c + vec2(-5.0, 5.0), c + vec2(5.0, -5.0)], st);
            }
            if resp.clicked() {
                if k == 0 {
                    out.0 = true;
                } else {
                    out.1 = true;
                }
            }
        }
    });
    out
}
