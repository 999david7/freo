//! Dialogs of the Werkzeuge tab in Creo's layout: Parameter, Beziehungen, Familientabelle,
//! Mitteilungsprotokoll, Modell-Player and the Informationsfenster. The remaining ones
//! (Referenz-Viewer, Such-Tool, …) are in `toolsdlg2`.

use super::creo::fieldset;
use super::dlgkit::*;
use super::icons::{self, Icon};
use super::ribbon::theme::*;
use super::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke, StrokeKind};

/// One rule of the parameter search: column, comparison, value, joined with ODER/UND.
#[derive(Clone)]
pub struct Rule {
    pub or: bool,
    pub col: usize,
    pub cmp: usize,
    pub val: String,
}

#[derive(Default)]
pub struct ParUi {
    /// parameters as edited in the open dialog (applied by OK)
    pub draft: Option<Vec<Parameter>>,
    pub sel: usize,
    pub designated: HashSet<String>,
    pub restricted: HashSet<String>,
    /// Parametereigenschaften: open tab
    pub props: Option<usize>,
    pub cols: bool,
    pub find: bool,
    pub filter: bool,
    pub scope: usize,
    pub filt: usize,
    pub main: usize,
    pub col_sel: usize,
    pub hid_sel: usize,
    /// columns shown, in order (empty: all nine)
    pub cols_shown: Vec<usize>,
    pub hide_filter: bool,
    pub rules: Vec<Rule>,
    pub rule_sel: Option<usize>,
    pub f_or: usize,
    pub f_col: usize,
    pub f_cmp: usize,
    pub f_val: String,
    pub extra_filters: Vec<String>,
}

impl ParUi {
    pub fn shown(&self) -> Vec<usize> {
        if self.cols_shown.is_empty() {
            (0..COLS.len()).collect()
        } else {
            self.cols_shown.clone()
        }
    }
    pub fn filters(&self) -> Vec<String> {
        FILTERS.iter().map(|s| s.to_string()).chain(self.extra_filters.iter().cloned()).collect()
    }
}

#[derive(Default)]
pub struct PlayerUi {
    pub regen: bool,
    pub each: bool,
    pub num: String,
    pub num_for: usize,
}

#[derive(Default)]
pub struct RelFind {
    pub find: String,
    pub repl: String,
    pub replace: bool,
    pub pos: usize,
}

#[derive(Default)]
pub struct ToolsUi {
    pub publish: bool,
    pub publish_tab: usize,
    pub publish_name: String,
    pub publish_sel: usize,
    /// Bemaßungen wechseln: dimensions shown by name
    pub dim_names: bool,
    /// UDF Menü-Manager (true: the DATENBANK menu is open)
    pub udf: Option<bool>,
    pub aux: bool,
    pub aux_apps: Vec<super::toolsdlg2::AuxApp>,
    pub aux_sel: Option<usize>,
    pub image: bool,
    pub image_ui: super::toolsdlg2::ImageUi,
    pub hatch: bool,
    pub thingmark: bool,
    /// Räumliches Ziel / ThingMark while its ribbon tab is open
    pub ar: Option<super::artarget::ArDlg>,
    /// Informationsfenster: title and text
    pub info: Option<(String, String)>,
    pub info_lines: bool,
    pub info_find: Option<String>,
    /// analyses run in this session (title, report) and the ones kept with „Analyse“
    pub analyses: Vec<(String, String)>,
    pub saved_analyses: Vec<(String, String)>,
    /// colours of the last analysis on the model
    pub overlay: Option<super::analysis::Overlay>,
    pub par: ParUi,
    pub rel_local: bool,
    pub rel_order: usize,
    pub rel_scope: usize,
    pub rel_units: bool,
    pub rel_hide: bool,
    pub rel_hist: Vec<String>,
    pub rel_redo: Vec<String>,
    pub rel_prev: Option<String>,
    pub rel_find: Option<RelFind>,
    /// Einfügen ▸ Funktion… (0) / Aus Liste… (1), and the chosen row
    pub rel_pick: Option<usize>,
    pub rel_pick_sel: usize,
    /// selection (char range) to apply to the relations editor
    pub rel_select: Option<(usize, usize)>,
    pub fam_edit: bool,
    pub fam_sysmass: bool,
    pub msg_lines: bool,
    pub msg_find: Option<String>,
    pub player: PlayerUi,
    pub refv: super::toolsdlg2::RefUi,
    pub find: super::toolsdlg2::FindUi,
    pub iface: super::toolsdlg2::IfaceUi,
    pub hatch_ui: super::toolsdlg2::HatchUi,
    pub farb: super::toolsdlg2::FarbUi,
    demo_done: bool,
    demo_wait: u8,
}

/// Functions of the relation language with what they return.
pub const FUNCS: [(&str, &str); 17] = [
    ("sin()", "Sinus (Winkel in Grad)"),
    ("cos()", "Kosinus (Winkel in Grad)"),
    ("tan()", "Tangens (Winkel in Grad)"),
    ("asin()", "Arkussinus in Grad"),
    ("acos()", "Arkuskosinus in Grad"),
    ("atan()", "Arkustangens in Grad"),
    ("sqrt()", "Quadratwurzel"),
    ("abs()", "Absolutwert"),
    ("min(,)", "Kleinerer von zwei Werten"),
    ("max(,)", "Größerer von zwei Werten"),
    ("floor()", "Abrunden auf ganze Zahl"),
    ("ceil()", "Aufrunden auf ganze Zahl"),
    ("round()", "Runden auf ganze Zahl"),
    ("log()", "Logarithmus zur Basis 10"),
    ("ln()", "Natürlicher Logarithmus"),
    ("exp()", "e hoch x"),
    ("pi", "Kreiszahl 3,14159…"),
];

/// `w` occurs in `hay` as a whole word (letters, digits and _ belong to a word); case is ignored.
pub fn has_word(hay: &str, w: &str) -> bool {
    let (h, w) = (hay.to_uppercase(), w.to_uppercase());
    if w.is_empty() {
        return false;
    }
    let is = |c: char| c.is_alphanumeric() || c == '_';
    let mut from = 0;
    while let Some(i) = h[from..].find(&w) {
        let a = from + i;
        let b = a + w.len();
        if !h[..a].chars().next_back().is_some_and(is) && !h[b..].chars().next().is_some_and(is) {
            return true;
        }
        from = b;
    }
    false
}

/// Assignment lines of the relations: (variable, right-hand side, whole line).
pub fn rel_lines(rel: &str) -> Vec<(String, String, String)> {
    rel.lines()
        .filter_map(|l| {
            let t = l.trim();
            if t.is_empty() || t.starts_with("/*") {
                return None;
            }
            let (a, b) = t.split_once('=')?;
            let a = a.trim();
            if a.is_empty() || !a.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return None;
            }
            Some((a.to_uppercase(), b.trim().to_string(), t.to_string()))
        })
        .collect()
}

/// Sorts the relations so that every value is computed before it is used; comments stay with
/// the line below them.
pub fn reorder_relations(rel: &str) -> String {
    let mut groups: Vec<Vec<String>> = vec![];
    let mut pending: Vec<String> = vec![];
    for l in rel.lines() {
        pending.push(l.to_string());
        if !l.trim().is_empty() && !l.trim().starts_with("/*") {
            groups.push(std::mem::take(&mut pending));
        }
    }
    let info: Vec<Option<(String, String)>> = groups.iter().map(|g| rel_lines(g.last().map(String::as_str).unwrap_or("")).into_iter().next().map(|(a, b, _)| (a, b))).collect();
    let n = groups.len();
    let mut done = vec![false; n];
    let mut out: Vec<String> = vec![];
    for _ in 0..n {
        // the first line whose inputs are all computed already
        let next = (0..n)
            .find(|&i| {
                !done[i]
                    && (0..n).all(|j| {
                        j == i
                            || done[j]
                            || match (&info[i], &info[j]) {
                                (Some((_, rhs)), Some((var, _))) => !has_word(rhs, var),
                                _ => true,
                            }
                    })
            })
            .or_else(|| (0..n).find(|&i| !done[i]));
        let Some(i) = next else { break };
        done[i] = true;
        out.extend(groups[i].iter().cloned());
    }
    out.extend(pending);
    out.join("\n")
}

const SCOPES: [&str; 13] =
    ["Teil", "KE", "Geerbt", "Anmerkungselement", "Sammelfläche", "Körper", "Fläche", "Kante", "Kurve", "Kurvenzug", "Material", "Eingebettetes Mathcad", "Volle Schweißnaht"];
const FILTERS: [&str; 5] =
    ["Standard", "Aktuelle und alle Unterebenen", "Aktuelles und alle Unter-KEs", "Aktuelle und alle Anmerkungs-KEs/-elemente", "Aktuelles und alle Anmerkungs-Unterelemente"];
const MAINS: [&str; 5] = ["Haupt", "Alternative Masseneigenschaften", "Gemeldete Masseneigenschaften", "Erweiterte gemeldete Masseneigenschaften", "Legacy"];
const COLS: [&str; 9] = ["Name", "Typ", "Wert", "Ausweisen", "Zugriff", "Quelle", "Beschreibung", "Eingeschränkt", "Einheitenmenge"];

fn par_menu() -> Vec<M> {
    vec![
        M::I("Parameter hinzufügen", "", true),
        M::I("Eingeschränkten Parametersatz hinzufügen...", "", false),
        M::I("Parameter löschen", "", true),
        M::Sep,
        M::I("Aufheben", "", true),
        M::Sep,
        M::Sub("Parameter sortieren", true, vec![M::I("Nach Name sortieren", "", true), M::I("Nach Wert sortieren", "", true), M::I("Nach Beschreibung sortieren", "", true)]),
        M::Sep,
        M::I("Parameterspalten...", "", true),
        M::I("Parametereigenschaften...", "", true),
    ]
}

/// Next free parameter name (P1, P2, …).
fn new_param(list: &[Parameter]) -> Parameter {
    let n = (1..).map(|k| format!("P{}", k)).find(|n| !list.iter().any(|p| &p.name == n)).unwrap();
    Parameter { name: n, expr: "10".into(), note: String::new() }
}

/// Handles the entries the Parameter menu shares between Parameter and Beziehungen.
fn par_action(t: &mut ParUi, hit: &str) -> bool {
    let Some(d) = t.draft.as_mut() else { return false };
    match hit {
        "Parameter hinzufügen" => {
            d.push(new_param(d));
            t.sel = d.len() - 1;
        }
        "Parameter löschen" => {
            if t.sel < d.len() {
                d.remove(t.sel);
                t.sel = t.sel.min(d.len().saturating_sub(1));
            }
        }
        "Aufheben" => {
            if let Some(p) = d.get(t.sel) {
                t.designated.remove(&p.name.to_uppercase());
            }
        }
        "Nach Name sortieren" => {
            d.sort_by_key(|p| p.name.to_uppercase());
            t.sel = 0;
        }
        "Nach Wert sortieren" => {
            d.sort_by(|a, b| match (a.expr.trim().parse::<f64>(), b.expr.trim().parse::<f64>()) {
                (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                _ => a.expr.cmp(&b.expr),
            });
            t.sel = 0;
        }
        "Nach Beschreibung sortieren" => {
            d.sort_by_key(|p| p.note.to_lowercase());
            t.sel = 0;
        }
        "Parameterspalten..." => t.cols = true,
        "Parametereigenschaften..." => {
            if t.sel < d.len() {
                t.props = Some(0);
            }
        }
        _ => return false,
    }
    true
}

/// "Suchen in": the object type, a pick button and the model.
fn search_in(ui: &mut egui::Ui, key: &str, scope: &mut usize, scopes: &[&str], model: &str) {
    fieldset(ui, "Suchen in", |ui| {
        ui.horizontal(|ui| {
            let w = ui.available_width();
            combo(ui, &format!("{}_scope", key), w * 0.36, scope, scopes, true, None);
            ibox(ui, Icon::TlCursor, false, "");
            let mut m = 0;
            combo(ui, &format!("{}_model", key), ui.available_width(), &mut m, &[model], true, Some(Icon::Part));
        });
    });
}

/// Text of one table cell, as the search rules compare it.
fn cell_text(t: &ParUi, p: &Parameter, col: usize) -> String {
    let key = p.name.to_uppercase();
    match col {
        0 => p.name.clone(),
        1 => "Reelle Zahl".into(),
        2 => p.expr.clone(),
        3 => {
            if t.designated.contains(&key) {
                "Ja".into()
            } else {
                "Nein".into()
            }
        }
        4 => "Voll".into(),
        5 => "Benutzerdefiniert".into(),
        6 => p.note.clone(),
        7 => {
            if t.restricted.contains(&key) {
                "Ja".into()
            } else {
                "Nein".into()
            }
        }
        _ => String::new(),
    }
}

/// Does parameter `p` satisfy the search rules (joined left to right with ODER / UND)?
fn rules_match(t: &ParUi, p: &Parameter) -> bool {
    let mut acc: Option<bool> = None;
    for r in &t.rules {
        let v = cell_text(t, p, r.col);
        let num = |s: &str| s.trim().replace(',', ".").parse::<f64>().ok();
        let m = match r.cmp {
            0 => super::toolsdlg2::glob(&r.val, &v),
            1 => !super::toolsdlg2::glob(&r.val, &v),
            2 => match (num(&v), num(&r.val)) {
                (Some(a), Some(b)) => a < b,
                _ => v.to_lowercase() < r.val.to_lowercase(),
            },
            _ => match (num(&v), num(&r.val)) {
                (Some(a), Some(b)) => a > b,
                _ => v.to_lowercase() > r.val.to_lowercase(),
            },
        };
        acc = Some(match acc {
            None => m,
            Some(a) => {
                if r.or {
                    a || m
                } else {
                    a && m
                }
            }
        });
    }
    acc.unwrap_or(false)
}

/// The parameter table of the Parameter and Beziehungen dialogs.
fn param_table(ui: &mut egui::Ui, t: &mut ParUi, h: f32) {
    const RH: f32 = 32.0;
    const GUT: f32 = 18.0;
    let cols = t.shown();
    let Some(list) = t.draft.as_mut() else { return };
    ui.horizontal(|ui| {
        let tw_ = ui.available_width() - 34.0;
        // the columns share the width of the table, so nothing scrolls sideways
        let cw = ((tw_ - GUT - 2.0) / cols.len().max(1) as f32).floor();
        egui::Frame::NONE.fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, LINE)).show(ui, |ui| {
            ui.set_width(tw_);
            ui.set_height(h + 28.0);
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            ui.vertical(|ui| {
                let total = GUT + cw * cols.len() as f32;
                let (hr, _) = ui.allocate_exact_size(vec2(total, 28.0), Sense::hover());
                let p = ui.painter().clone();
                p.rect_filled(hr, 0.0, Color32::from_rgb(246, 247, 248));
                p.rect_filled(Rect::from_min_size(hr.min, vec2(GUT, 28.0)), 0.0, Color32::from_rgb(214, 218, 222));
                for (k, c) in cols.iter().enumerate() {
                    let cr = Rect::from_min_size(pos2(hr.left() + GUT + cw * k as f32, hr.top()), vec2(cw, 28.0));
                    p.with_clip_rect(cr.shrink2(vec2(2.0, 0.0))).text(pos2(cr.left() + 10.0, cr.center().y), Align2::LEFT_CENTER, COLS[*c], font(), TEXT);
                    p.line_segment([cr.right_top(), cr.right_bottom()], Stroke::new(1.0_f32, Color32::from_rgb(222, 225, 228)));
                }
                p.line_segment([hr.left_bottom(), hr.right_bottom()], Stroke::new(1.0_f32, LINE));
                egui::ScrollArea::vertical().id_salt("par_v").auto_shrink([false, false]).max_height(h).show(ui, |ui| {
                    for i in 0..list.len() {
                        let (rr, resp) = ui.allocate_exact_size(vec2(total, RH), Sense::click());
                        if resp.clicked() {
                            t.sel = i;
                        }
                        let p = ui.painter().clone();
                        p.rect_filled(Rect::from_min_size(rr.min, vec2(GUT, RH)), 0.0, Color32::from_rgb(214, 218, 222));
                        let cell = |k: usize| Rect::from_min_size(pos2(rr.left() + GUT + cw * k as f32, rr.top()), vec2(cw, RH));
                        p.line_segment([rr.left_bottom(), rr.right_bottom()], Stroke::new(1.0_f32, Color32::from_rgb(232, 234, 236)));
                        let key = list[i].name.to_uppercase();
                        for (k, c) in cols.iter().enumerate() {
                            let r = cell(k);
                            if k == 0 && t.sel == i {
                                p.rect_filled(r, 0.0, ROW_SEL);
                            }
                            p.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0_f32, Color32::from_rgb(232, 234, 236)));
                            let text = |s: &str| {
                                p.with_clip_rect(r.shrink2(vec2(3.0, 0.0))).text(
                                    pos2(r.left() + 6.0, rr.center().y),
                                    Align2::LEFT_CENTER,
                                    s,
                                    font(),
                                    Color32::from_rgb(140, 144, 150),
                                );
                            };
                            match *c {
                                0 | 2 | 6 => {
                                    let mut child =
                                        ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(vec2(4.0, 5.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
                                    let s = match *c {
                                        0 => &mut list[i].name,
                                        2 => &mut list[i].expr,
                                        _ => &mut list[i].note,
                                    };
                                    let e = child.add(egui::TextEdit::singleline(s).id(ui.id().with(("par_cell", i, *c))).frame(false).font(font()).desired_width(cw - 10.0));
                                    if e.has_focus() {
                                        t.sel = i;
                                    }
                                }
                                1 => text("Reelle Zahl"),
                                3 | 7 => {
                                    let set = if *c == 3 { &mut t.designated } else { &mut t.restricted };
                                    super::creo::check_box(&p, Rect::from_center_size(r.center(), vec2(15.0, 15.0)), set.contains(&key));
                                    if ui.interact(r, ui.id().with(("par_chk", i, *c)), Sense::click()).clicked() {
                                        if !set.remove(&key) {
                                            set.insert(key.clone());
                                        }
                                        t.sel = i;
                                    }
                                }
                                4 => {
                                    icons::paint(&p, Rect::from_center_size(pos2(r.left() + 14.0, rr.center().y), vec2(16.0, 16.0)), Icon::TlLock, true);
                                    p.with_clip_rect(r).text(pos2(r.left() + 26.0, rr.center().y), Align2::LEFT_CENTER, "Voll", font(), TEXT);
                                }
                                5 => text("Benutzerdefiniert"),
                                _ => {}
                            }
                        }
                    }
                });
            });
        });
        ui.vertical(|ui| {
            ui.add_space(h * 0.5 - 6.0);
            let n = list.len();
            if tool(ui, Icon::TlUp, t.sel > 0 && n > 1, "").clicked() {
                list.swap(t.sel, t.sel - 1);
                t.sel -= 1;
            }
            if tool(ui, Icon::TlDown, t.sel + 1 < n, "").clicked() {
                list.swap(t.sel, t.sel + 1);
                t.sel += 1;
            }
        });
    });
}

/// + and −, the parameter set, Eigenschaften…, columns and search under the table.
fn param_footer(ui: &mut egui::Ui, t: &mut ParUi, key: &str) {
    ui.horizontal(|ui| {
        if ibox(ui, Icon::TlPlus, true, "Neuen Parameter hinzufügen").clicked() {
            par_action(t, "Parameter hinzufügen");
        }
        if ibox(ui, Icon::TlMinus, t.draft.as_ref().is_some_and(|d| !d.is_empty()), "Gewählten Parameter löschen").clicked() {
            par_action(t, "Parameter löschen");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ibox(ui, Icon::TlBinoculars, true, "Parameter suchen").clicked() {
                t.find = true;
            }
            if ibox(ui, Icon::TlColumns, true, "Spalten der Parametertabelle einrichten").clicked() {
                t.cols = true;
            }
            if btn(ui, "Eigenschaften...", 110.0, 0) {
                par_action(t, "Parametereigenschaften...");
            }
            combo(ui, &format!("{}_main", key), 250.0, &mut t.main, &MAINS, true, None);
        });
    });
}

fn bottom_buttons(ui: &mut egui::Ui, reset: bool) -> (bool, bool, bool) {
    let mut out = (false, false, false);
    hline(ui);
    pad(ui, 10.0, |ui| {
        ui.horizontal(|ui| {
            if reset && btn(ui, "Zurücksetzen", 100.0, 0) {
                out.2 = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if btn(ui, "Abbrechen", 120.0, 0) {
                    out.1 = true;
                }
                if btn(ui, "OK", 120.0, 1) {
                    out.0 = true;
                }
            });
        });
    });
    out
}

/// A row of a find bar: the search text and the number of hits.
fn find_bar(ui: &mut egui::Ui, find: &mut Option<String>, hits: usize) {
    let Some(q) = find else { return };
    let mut close = false;
    egui::Frame::NONE.fill(Color32::from_rgb(232, 236, 240)).inner_margin(4.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            lbl(ui, "Suchen:");
            field(ui, q, 240.0, true);
            lbl(ui, &if q.is_empty() { String::new() } else { format!("{} Treffer", hits) });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if btn(ui, "Schließen", 80.0, 0) {
                    close = true;
                }
            });
        });
    });
    if close {
        *find = None;
    }
}

impl App {
    pub(super) fn todo(&mut self, name: &str) {
        self.say(format!("„{}“ ist in Freo noch nicht verfügbar.", name.trim_end_matches("...")), MsgKind::Info);
    }

    /// Folder of the model file, or the working directory for a model that was never saved.
    pub(super) fn work_dir(&self) -> PathBuf {
        self.path.as_ref().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_else(|| self.browser_dir.clone())
    }

    /// Asks for a file name and writes `text` there.
    pub(super) fn save_text(&mut self, title: &str, name: &str, text: &str) {
        let Some(p) = rfd::FileDialog::new().set_title(title).set_directory(self.work_dir()).set_file_name(name).save_file() else { return };
        match std::fs::write(&p, text) {
            Ok(_) => self.say(format!("Information in Datei {} geschrieben.", p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()), MsgKind::Info),
            Err(e) => self.say(format!("Datei konnte nicht geschrieben werden: {}", e), MsgKind::Error),
        }
    }

    /// Asks for a file and returns its text.
    pub(super) fn load_text(&mut self, title: &str) -> Option<String> {
        let p = rfd::FileDialog::new().set_title(title).set_directory(self.work_dir()).pick_file()?;
        match std::fs::read_to_string(&p) {
            Ok(s) => Some(s),
            Err(e) => {
                self.say(format!("Datei konnte nicht gelesen werden: {}", e), MsgKind::Error);
                None
            }
        }
    }

    /// Where a parameter is used: relations that set it, relations and features that read it.
    fn usage(&self, name: &str, relations: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
        let lines = rel_lines(relations);
        let driven: Vec<String> = lines.iter().filter(|l| l.0 == name.to_uppercase()).map(|l| l.2.clone()).collect();
        let driving: Vec<String> = lines.iter().filter(|l| has_word(&l.1, name)).map(|l| l.2.clone()).collect();
        let feats: Vec<String> =
            self.data.features.iter().filter(|f| serde_json::to_string(&f.kind).map(|j| has_word(&j, name)).unwrap_or(false)).map(|f| f.name.to_uppercase()).collect();
        (driven, driving, feats)
    }

    /// Werkzeuge ▸ KE-Informationen: the selected feature in the Informationsfenster.
    pub fn feature_info(&mut self) {
        let Some(f) = self.tree_selection.first().and_then(|id| self.data.feature(*id)).cloned() else {
            self.say("KE oder Komponente auswählen.", MsgKind::Prompt);
            return;
        };
        let no = self.data.features.iter().position(|x| x.id == f.id).map(|i| i + 1).unwrap_or(0);
        let name = |id: &u64| self.data.feature(*id).map(|p| p.name.clone());
        let parents: Vec<String> = f.kind.references().iter().filter_map(name).collect();
        let children: Vec<String> = self.data.features.iter().filter(|c| c.kind.references().contains(&f.id)).map(|c| c.name.clone()).collect();
        let list = |v: &[String]| if v.is_empty() { "  (keine)".to_string() } else { v.iter().map(|s| format!("  {}", s)).collect::<Vec<_>>().join("\n") };
        let text = format!("KE-INFORMATIONEN\n================================================\n\nModell:      {}\nKE-Nummer:   {}\nInterne ID:  {}\nName:        {}\nTyp:         {}\nStatus:      {}\n\nELTERN\n{}\n\nKINDER\n{}\n",
            self.data.name.to_uppercase(), no, f.id, f.name.to_uppercase(), f.kind.type_name(), if f.suppressed { "Unterdrückt" } else { "Regeneriert" }, list(&parents), list(&children));
        self.creo.tools.info = Some((format!("Informationsfenster ({}.inf)", self.data.name.to_lowercase()), text));
    }

    /// Werkzeuge ▸ KE-Liste: every feature of the model in the Informationsfenster.
    pub fn feature_list(&mut self) {
        let mut text = format!(
            "KE-LISTE FÜR TEIL {}\n================================================\n\n{:<5}{:<8}{:<28}{:<26}{}\n",
            self.data.name.to_uppercase(),
            "Nr.",
            "ID",
            "Name",
            "Typ",
            "Status"
        );
        for (i, f) in self.data.features.iter().enumerate() {
            text += &format!("{:<5}{:<8}{:<28}{:<26}{}\n", i + 1, f.id, f.name.to_uppercase(), f.kind.type_name(), if f.suppressed { "Unterdrückt" } else { "Regeneriert" });
        }
        self.creo.tools.info = Some(("Informationsfenster (feature.lst)".into(), text));
        self.say("Information in Datei feature.lst geschrieben.", MsgKind::Info);
    }

    /// Reports of the Werkzeuge tab's overflow menus (Untersuchen ▾, Modellabsicht ▾).
    pub fn tools_report(&mut self, what: &str) {
        let model = self.model_label();
        let head = |t: &str| format!("{} FÜR TEIL {}\n================================================\n\n", t, model);
        let info = |name: &str, text: String| Some((format!("Informationsfenster ({})", name), text));
        match what {
            "Dateiverlauf" => {
                let mut text = head("DATEIVERLAUF");
                match &self.path {
                    Some(p) => {
                        text += &format!("Datei:        {}\n", p.display());
                        if let Ok(m) = std::fs::metadata(p) {
                            text += &format!("Größe:        {} Bytes\n", m.len());
                            if let Ok(age) = m.modified().and_then(|t| t.elapsed().map_err(std::io::Error::other)) {
                                let s = age.as_secs();
                                text += &format!(
                                    "Gespeichert:  vor {}\n",
                                    if s < 120 {
                                        format!("{} Sekunden", s)
                                    } else if s < 7200 {
                                        format!("{} Minuten", s / 60)
                                    } else if s < 172800 {
                                        format!("{} Stunden", s / 3600)
                                    } else {
                                        format!("{} Tagen", s / 86400)
                                    }
                                );
                            }
                        }
                    }
                    None => text += "Das Modell wurde noch nicht gespeichert.\n",
                }
                text += &format!("Status:       {}\n", if self.dirty { "Seit dem letzten Speichern geändert" } else { "Unverändert" });
                self.creo.tools.info = info("verlauf.inf", text);
            }
            "Import-Protokolldatei" => {
                let mut text = head("IMPORT-PROTOKOLL");
                let imports: Vec<String> = self
                    .data
                    .features
                    .iter()
                    .filter_map(|f| match &f.kind {
                        FeatureKind::Import(d) => Some(format!("{:<24}Quelle: {}   Format: {}   Flächen: {}", f.name.to_uppercase(), d.source, d.format, d.faces.len())),
                        _ => None,
                    })
                    .collect();
                text += &if imports.is_empty() { "Das Modell enthält keine importierten KEs.\n".to_string() } else { imports.join("\n") + "\n" };
                self.creo.tools.info = info("import.log", text);
            }
            "Änderungen anzeigen" => {
                let mut text = head("ÄNDERUNGEN");
                text += &format!(
                    "Status:                  {}\nSchritte zum Widerrufen: {}\n",
                    if self.dirty { "Seit dem letzten Speichern geändert" } else { "Keine ungespeicherten Änderungen" },
                    self.undo.len()
                );
                // features that the last saved-to-undo state does not have, or has differently
                if let Some(prev) = self.undo.first() {
                    text += "\nSeit Beginn der Sitzung geändert:\n";
                    let mut n = 0;
                    for f in &self.data.features {
                        match prev.feature(f.id) {
                            None => {
                                text += &format!("  + {} (neu)\n", f.name.to_uppercase());
                                n += 1;
                            }
                            Some(o) if o != f => {
                                text += &format!("  * {} (geändert)\n", f.name.to_uppercase());
                                n += 1;
                            }
                            _ => {}
                        }
                    }
                    for o in &prev.features {
                        if self.data.feature(o.id).is_none() {
                            text += &format!("  - {} (gelöscht)\n", o.name.to_uppercase());
                            n += 1;
                        }
                    }
                    if n == 0 {
                        text += "  (keine Konstruktionselemente)\n";
                    }
                }
                self.creo.tools.info = info("aenderungen.inf", text);
            }
            "Modellgröße" => {
                let b = self.result.topo.bbox;
                let m = export::mass_properties(&self.result, self.data.density);
                let s = b.size();
                let text = head("MODELLGRÖSSE") + &format!("Begrenzungsrahmen:  {} × {} × {} mm\nDiagonale:          {} mm\nVolumen:            {} mm³\nOberfläche:         {} mm²\nKonstruktionselemente: {}\nFlächen:            {}\n",
                    fmt_num(s.x, 3), fmt_num(s.y, 3), fmt_num(s.z, 3), fmt_num(b.diagonal(), 3), fmt_num(m.volume, 3), fmt_num(m.area, 3), self.data.features.len(), self.result.topo.faces.len());
                self.creo.tools.info = info("groesse.inf", text);
            }
            "Programm" => {
                let mut text = format!("VERSION 10.0\nLISTING FÜR TEIL {}\n\nINPUT\n", model);
                for p in &self.data.parameters {
                    text += &format!("  {} NUMBER\n", p.name.to_uppercase());
                }
                text += "END INPUT\n\nRELATIONS\n";
                for l in self.data.relations.lines() {
                    text += &format!("{}\n", l);
                }
                text += "END RELATIONS\n\n";
                for (i, f) in self.data.features.iter().enumerate() {
                    text += &format!(
                        "ADD FEATURE (initial number {})\nINTERNAL FEATURE ID {}\nNAME = {}\nTYPE = {}{}\nEND ADD\n\n",
                        i + 1,
                        f.id,
                        f.name.to_uppercase(),
                        f.kind.type_name().to_uppercase(),
                        if f.suppressed { "\nSUPPRESSED" } else { "" }
                    );
                }
                text += "MASSPROP\nEND MASSPROP\n";
                self.creo.tools.info = info(&format!("{}.pls", self.data.name.to_lowercase()), text);
            }
            "Ausweisen" => {
                self.windows.parameters = true;
                self.say("Zu veröffentlichende Parameter in der Spalte „Ausweisen“ markieren.", MsgKind::Prompt);
            }
            "Beziehungen und Parameter" => {
                let mut text = head("BEZIEHUNGEN UND PARAMETER") + "Beziehungen:\n";
                text +=
                    &if self.data.relations.trim().is_empty() { "  (keine)\n".to_string() } else { self.data.relations.lines().map(|l| format!("  {}\n", l)).collect::<String>() };
                text += &format!("\nParameter:\n  {:<20}{:<14}{:<18}{}\n", "Name", "Typ", "Wert", "Beschreibung");
                for p in &self.data.parameters {
                    let v = self.result.vars.get(&p.name.to_uppercase()).map(|v| fmt_num(*v, 4)).unwrap_or_else(|| p.expr.clone());
                    text += &format!("  {:<20}{:<14}{:<18}{}\n", p.name.to_uppercase(), "Reelle Zahl", v, p.note);
                }
                self.creo.tools.info = info("rel.inf", text);
            }
            "Deklarieren" => self.say("Deklarieren: In dieser Sitzung ist kein Notizbuch geöffnet, dem das Modell zugeordnet werden kann.", MsgKind::Warning),
            "Nach Geometrie" => {
                let Some(p) =
                    rfd::FileDialog::new().set_title("Teil für den Geometrievergleich wählen").set_directory(self.work_dir()).add_filter("Freo-Teil", &["pfprt"]).pick_file()
                else {
                    return;
                };
                let other = match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<ModelData>(&s).map_err(|e| e.to_string())) {
                    Ok(d) => d,
                    Err(e) => {
                        self.say(format!("Teil konnte nicht geladen werden: {}", e), MsgKind::Error);
                        return;
                    }
                };
                let r2 = regen::run(&other);
                let (a, b) = (export::mass_properties(&self.result, self.data.density), export::mass_properties(&r2, other.density));
                let (sa, sb) = (self.result.topo.bbox.size(), r2.topo.bbox.size());
                let row = |n: &str, x: f64, y: f64| format!("{:<22}{:>16}{:>16}{:>16}\n", n, fmt_num(x, 3), fmt_num(y, 3), fmt_num(y - x, 3));
                let mut text = format!(
                    "GEOMETRIEVERGLEICH\n================================================\n\n{:<22}{:>16}{:>16}{:>16}\n",
                    "",
                    model,
                    other.name.to_uppercase(),
                    "Differenz"
                );
                text += &row("Volumen (mm³)", a.volume, b.volume);
                text += &row("Oberfläche (mm²)", a.area, b.area);
                text += &row("Länge X (mm)", sa.x, sb.x);
                text += &row("Länge Y (mm)", sa.y, sb.y);
                text += &row("Länge Z (mm)", sa.z, sb.z);
                text += &row("Flächen", self.result.topo.faces.len() as f64, r2.topo.faces.len() as f64);
                let same = (a.volume - b.volume).abs() < 1e-6 * a.volume.abs().max(1.0) && (a.area - b.area).abs() < 1e-6 * a.area.abs().max(1.0) && (sa - sb).length() < 1e-6;
                text += &format!("\nErgebnis: {}\n", if same { "Die Geometrie der beiden Teile stimmt überein." } else { "Die Geometrie der beiden Teile unterscheidet sich." });
                self.creo.tools.info = info("vergleich.inf", text);
            }
            n => self.todo(n),
        }
    }

    /// All dialogs of the Werkzeuge tab.
    pub fn tools_dialogs(&mut self, ctx: &egui::Context) {
        let mut t = std::mem::take(&mut self.creo.tools);
        if !t.demo_done {
            t.demo_wait += 1;
        }
        if !t.demo_done && t.demo_wait > 6 {
            t.demo_done = true;
            // FREO_DEMO_DLG=<name> (or FREO_DEMO_POPUP=dlg:<name>) opens a dialog for documentation screenshots
            if let Some(d) = std::env::var("FREO_DEMO_DLG").ok().or_else(|| std::env::var("FREO_DEMO_POPUP").ok().and_then(|v| v.strip_prefix("dlg:").map(str::to_string))) {
                match d.as_str() {
                    "params" => self.windows.parameters = true,
                    "parprops" | "parprops1" | "parprops2" => {
                        self.windows.parameters = true;
                        t.par.props = Some(match d.as_str() {
                            "parprops1" => 1,
                            "parprops2" => 2,
                            _ => 0,
                        });
                    }
                    "parcols" => {
                        self.windows.parameters = true;
                        t.par.cols = true;
                    }
                    "parfind" => {
                        self.windows.parameters = true;
                        t.par.find = true;
                    }
                    "parfilter" => {
                        self.windows.parameters = true;
                        t.par.filter = true;
                    }
                    "relations" => self.windows.relations = true,
                    "relations2" => {
                        self.windows.relations = true;
                        t.rel_local = true;
                    }
                    "relfind" => {
                        self.windows.relations = true;
                        t.rel_find = Some(RelFind { find: "BREITE".into(), replace: true, ..Default::default() });
                    }
                    "relpick" => {
                        self.windows.relations = true;
                        t.rel_pick = Some(0);
                    }
                    "family" => self.windows.family = true,
                    "refviewer" | "refviewer1" => {
                        self.creo.ref_viewer = true;
                        if d == "refviewer1" {
                            t.refv.tab = 1;
                        }
                    }
                    "refviewer2" => {
                        self.creo.ref_viewer = true;
                        self.tree_selection = self.data.features.iter().filter(|f| f.name.starts_with("Bohrung")).take(1).map(|f| f.id).collect();
                    }
                    "search" => self.creo.search = Some("*".into()),
                    "messages" => self.windows.messages = true,
                    "player" => self.creo.player = true,
                    "iface" => self.creo.iface_dialog = true,
                    "publish" => t.publish = true,
                    "publish1" => {
                        t.publish = true;
                        t.publish_tab = 1;
                    }
                    "udf" => t.udf = Some(false),
                    "udf1" => t.udf = Some(true),
                    "aux" => t.aux = true,
                    "image" => t.image = true,
                    "hatch" => t.hatch = true,
                    "farb" => self.windows.appearance = true,
                    "ar" | "thingmark" => {
                        t.thingmark = d == "thingmark";
                        self.creo.tools = t;
                        self.start_ar_target();
                        t = std::mem::take(&mut self.creo.tools);
                        t.demo_done = true;
                        t.demo_wait = 9;
                    }
                    "dimnames" => t.dim_names = true,
                    // "anc:<name>": the analysis colours without the report window (screenshots)
                    d if d.starts_with("anc:") => {
                        let n = d[4..].to_string();
                        self.creo.tools = t;
                        self.run_analysis(&n);
                        t = std::mem::take(&mut self.creo.tools);
                        t.info = None;
                        self.selection.clear();
                        t.demo_done = true;
                        t.demo_wait = 9;
                    }
                    d if d.starts_with("an:") => {
                        let n = d[3..].to_string();
                        self.creo.tools = t;
                        self.run_analysis(&n);
                        t = std::mem::take(&mut self.creo.tools);
                        t.demo_done = true;
                        t.demo_wait = 9;
                    }
                    "info" => {
                        self.creo.tools = t;
                        self.feature_list();
                        t = std::mem::take(&mut self.creo.tools);
                        t.demo_done = true;
                        t.demo_wait = 9;
                    }
                    _ => {}
                }
                if d.starts_with("relations") || d.starts_with("rel") {
                    if self.data.relations.is_empty() {
                        self.data.relations = "/* Abmessungen der Grundplatte\nHOEHE = BREITE / 3\nANZAHL = ceil(BREITE / 25)".into();
                    }
                }
            }
        }
        super::dims3d::DIM_NAMES.store(t.dim_names, std::sync::atomic::Ordering::Relaxed);
        if (self.windows.parameters || self.windows.relations) && t.par.draft.is_none() {
            t.par.draft = Some(self.data.parameters.clone());
            t.par.sel = 0;
            t.par.designated = self.data.designated.iter().cloned().collect();
            t.par.restricted = self.data.restricted.iter().cloned().collect();
        }
        if !(self.windows.parameters || self.windows.relations) {
            t.par.draft = None;
            t.par.props = None;
            t.par.cols = false;
            t.par.find = false;
            t.par.filter = false;
        }
        self.dlg_parameters(ctx, &mut t);
        self.dlg_relations(ctx, &mut t);
        self.dlg_par_sub(ctx, &mut t);
        self.dlg_family(ctx, &mut t);
        self.dlg_messages(ctx, &mut t);
        self.dlg_player(ctx, &mut t);
        self.dlg_ref_viewer(ctx, &mut t);
        self.dlg_search(ctx, &mut t);
        self.dlg_interface(ctx, &mut t);
        self.dlg_publish(ctx, &mut t);
        self.dlg_udf(ctx, &mut t);
        self.dlg_aux(ctx, &mut t);
        self.dlg_image(ctx, &mut t);
        self.dlg_hatch(ctx, &mut t);
        self.dlg_appearance(ctx, &mut t);
        self.dlg_info(ctx, &mut t);
        // a dialog may have opened the Informationsfenster through `self`
        if let Some(i) = self.creo.tools.info.take() {
            t.info = Some(i);
        }
        self.creo.tools = t;
    }

    pub(super) fn model_label(&self) -> String {
        self.data.name.to_uppercase()
    }

    fn apply_params(&mut self, t: &mut ToolsUi, relations: Option<String>) {
        let params = t.par.draft.clone().unwrap_or_else(|| self.data.parameters.clone());
        let rel = relations.unwrap_or_else(|| self.data.relations.clone());
        // Ausweisen / Eingeschränkt are saved with the model, for the parameters that still exist
        let names: HashSet<String> = params.iter().map(|p| p.name.to_uppercase()).collect();
        let keep = |s: &HashSet<String>| {
            let mut v: Vec<String> = s.iter().filter(|n| names.contains(*n)).cloned().collect();
            v.sort();
            v
        };
        let (des, res) = (keep(&t.par.designated), keep(&t.par.restricted));
        if params != self.data.parameters || rel != self.data.relations || des != self.data.designated || res != self.data.restricted {
            self.commit("Parameter und Beziehungen aktualisiert.", |d| {
                d.parameters = params;
                d.relations = rel;
                d.designated = des;
                d.restricted = res;
            });
        }
        for e in self.result.param_errors.clone() {
            self.say(e, MsgKind::Error);
        }
    }

    /// Parameter table as text: `sep` between the columns.
    fn params_text(&self, t: &ToolsUi, sep: &str) -> String {
        let mut s = ["Name", "Typ", "Wert", "Ausweisen", "Beschreibung"].join(sep) + "\n";
        for p in t.par.draft.as_deref().unwrap_or(&self.data.parameters) {
            s += &[
                p.name.clone(),
                "Reelle Zahl".into(),
                p.expr.clone(),
                if t.par.designated.contains(&p.name.to_uppercase()) { "Ja".into() } else { "Nein".into() },
                p.note.clone(),
            ]
            .join(sep);
            s.push('\n');
        }
        s
    }

    /// Reads parameters from a CSV or text table and merges them into the draft.
    fn import_params(&mut self, t: &mut ToolsUi, text: &str) {
        let Some(d) = t.par.draft.as_mut() else { return };
        let mut n = 0;
        for line in text.lines() {
            let f: Vec<&str> = if line.contains(';') {
                line.split(';').collect()
            } else if line.contains('\t') {
                line.split('\t').collect()
            } else {
                line.split_whitespace().collect()
            };
            let f: Vec<&str> = f.iter().map(|x| x.trim()).collect();
            if f.len() < 2 || f[0].is_empty() || f[0].eq_ignore_ascii_case("name") {
                continue;
            }
            // name;typ;wert;ausweisen;beschreibung — or just name;wert
            let (val, des, note) = if f.len() >= 5 {
                (f[2], f[3].eq_ignore_ascii_case("ja"), f[4])
            } else if f.len() >= 3 {
                (f[2], false, "")
            } else {
                (f[1], false, "")
            };
            match d.iter_mut().find(|p| p.name.eq_ignore_ascii_case(f[0])) {
                Some(p) => {
                    p.expr = val.to_string();
                    if !note.is_empty() {
                        p.note = note.to_string();
                    }
                }
                None => d.push(Parameter { name: f[0].to_string(), expr: val.to_string(), note: note.to_string() }),
            }
            if des {
                t.par.designated.insert(f[0].to_uppercase());
            }
            n += 1;
        }
        self.say(format!("{} Parameter importiert.", n), MsgKind::Info);
    }

    fn table_config(t: &ToolsUi) -> String {
        format!(
            "filter={}\nhaupt={}\nspalten={}\nfilterzeile={}\n",
            t.par.filt,
            t.par.main,
            t.par.shown().iter().map(|c| c.to_string()).collect::<Vec<_>>().join(","),
            if t.par.hide_filter { 0 } else { 1 }
        )
    }

    fn load_table_config(t: &mut ToolsUi, text: &str) {
        for l in text.lines() {
            let Some((k, v)) = l.split_once('=') else { continue };
            match k.trim() {
                "filter" => t.par.filt = v.trim().parse().unwrap_or(0).min(t.par.filters().len() - 1),
                "haupt" => t.par.main = v.trim().parse().unwrap_or(0).min(MAINS.len() - 1),
                "spalten" => {
                    let c: Vec<usize> = v.split(',').filter_map(|x| x.trim().parse().ok()).filter(|c| *c < COLS.len()).collect();
                    if c.first() == Some(&0) {
                        t.par.cols_shown = c;
                    }
                }
                "filterzeile" => t.par.hide_filter = v.trim() == "0",
                _ => {}
            }
        }
    }

    /// Menu entries the Parameter and Beziehungen dialogs share. Returns false for other entries.
    fn par_menu_hit(&mut self, ctx: &egui::Context, t: &mut ToolsUi, h: &str, relations: &str) -> bool {
        let model = self.data.name.to_lowercase();
        let cur = t.par.draft.as_ref().and_then(|d| d.get(t.par.sel)).cloned();
        match h {
            "CSV-Datei exportieren..." => {
                let s = self.params_text(t, ";");
                self.save_text("Parameter exportieren", &format!("{}_parameter.csv", model), &s);
            }
            "Textdatei exportieren..." => {
                let s = self.params_text(t, "\t");
                self.save_text("Parameter exportieren", &format!("{}_parameter.txt", model), &s);
            }
            "CSV-Datei importieren..." | "Textdatei importieren..." => {
                if let Some(s) = self.load_text("Parameter importieren") {
                    self.import_params(t, &s);
                }
            }
            "Konfiguration speichern als..." => {
                let s = Self::table_config(t);
                self.save_text("Konfiguration speichern", "parametertabelle.cfg", &s);
            }
            "Konfiguration laden..." => {
                if let Some(s) = self.load_text("Konfiguration laden") {
                    Self::load_table_config(t, &s);
                    self.say("Konfiguration der Parametertabelle geladen.", MsgKind::Info);
                }
            }
            "Filter ausblenden" => t.par.hide_filter = !t.par.hide_filter,
            "Ungültige Werte" => {
                let mut d = self.data.clone();
                if let Some(p) = &t.par.draft {
                    d.parameters = p.clone();
                }
                d.relations = relations.to_string();
                let (_, errs) = d.evaluate_parameters();
                let text = format!(
                    "UNGÜLTIGE WERTE\n================================================\n\n{}\n",
                    if errs.is_empty() { "Alle Parameterwerte sind gültig.".to_string() } else { errs.join("\n") }
                );
                t.info = Some(("Informationsfenster (ungueltig.inf)".into(), text));
            }
            "Konfliktbericht" => {
                let list = t.par.draft.clone().unwrap_or_default();
                let mut out = vec![];
                for (i, p) in list.iter().enumerate() {
                    if list[..i].iter().any(|q| q.name.eq_ignore_ascii_case(&p.name)) {
                        out.push(format!("{}: Name mehrfach vergeben.", p.name.to_uppercase()));
                    }
                    if rel_lines(relations).iter().any(|l| l.0 == p.name.to_uppercase()) {
                        out.push(format!("{}: Wert wird zusätzlich durch eine Beziehung gesetzt.", p.name.to_uppercase()));
                    }
                }
                let text = format!(
                    "KONFLIKTBERICHT\n================================================\n\n{}\n",
                    if out.is_empty() { "Keine Konflikte gefunden.".to_string() } else { out.join("\n") }
                );
                t.info = Some(("Informationsfenster (konflikt.inf)".into(), text));
            }
            "Einschränkungsdefinition aktualisieren" => {
                let n = t.par.restricted.len();
                self.say(format!("Einschränkungsdefinitionen aktualisiert ({} eingeschränkte Parameter).", n), MsgKind::Info);
            }
            "Verwendungsnachweis" | "Gesteuert durch..." | "Steuernd..." => {
                let Some(p) = cur else {
                    self.say("Einen Parameter in der Tabelle auswählen.", MsgKind::Prompt);
                    return true;
                };
                let (driven, driving, feats) = self.usage(&p.name, relations);
                let list = |v: &[String]| if v.is_empty() { "  (keine)".to_string() } else { v.iter().map(|s| format!("  {}", s)).collect::<Vec<_>>().join("\n") };
                let name = p.name.to_uppercase();
                let text = match h {
                    "Gesteuert durch..." => format!("PARAMETER {} WIRD GESTEUERT DURCH\n================================================\n\nBeziehungen:\n{}\n", name, list(&driven)),
                    "Steuernd..." => format!("PARAMETER {} STEUERT\n================================================\n\nBeziehungen:\n{}\n\nKonstruktionselemente:\n{}\n", name, list(&driving), list(&feats)),
                    _ => format!("VERWENDUNGSNACHWEIS FÜR PARAMETER {}\n================================================\n\nGesetzt durch Beziehungen:\n{}\n\nVerwendet in Beziehungen:\n{}\n\nVerwendet in Konstruktionselementen:\n{}\n", name, list(&driven), list(&driving), list(&feats)),
                };
                t.info = Some(("Informationsfenster (verwendung.inf)".into(), text));
            }
            "Kopieren" if !self.windows.relations || self.windows.parameters => {
                if let Some(p) = cur {
                    ctx.copy_text(format!("{}\t{}\t{}", p.name, p.expr, p.note));
                    self.say(format!("Parameter {} in die Zwischenablage kopiert.", p.name.to_uppercase()), MsgKind::Info);
                }
            }
            h => return par_action(&mut t.par, h),
        }
        true
    }

    fn dlg_parameters(&mut self, ctx: &egui::Context, t: &mut ToolsUi) {
        if !self.windows.parameters {
            return;
        }
        let model = self.model_label();
        let filters = t.par.filters();
        let filters: Vec<&str> = filters.iter().map(String::as_str).collect();
        let (mut ok, mut cancel, mut reset, mut hit) = (false, false, false, None);
        let open = cwin(ctx, "dlg_par", "Parameter", true, pos2(250.0, 96.0), 924.0, |ui| {
            hit = menubar(
                ui,
                "par",
                &[
                    (
                        "Datei",
                        vec![
                            M::Sub("Daten importieren", true, vec![M::I("CSV-Datei importieren...", "", true), M::I("Textdatei importieren...", "", true)]),
                            M::Sub("Daten exportieren", true, vec![M::I("CSV-Datei exportieren...", "", true), M::I("Textdatei exportieren...", "", true)]),
                            M::Sep,
                            M::I("Konfiguration speichern als...", "", true),
                            M::I("Konfiguration laden...", "", true),
                        ],
                    ),
                    (
                        "Editieren",
                        vec![
                            M::I("Ausschneiden", "Strg+X", false),
                            M::I("Kopieren", "Strg+C", true),
                            M::I("Einfügen", "Strg+V", false),
                            M::I("Suchen", "Strg+F", true),
                            M::I("Filter ausblenden", "", true),
                        ],
                    ),
                    ("Parameter", par_menu()),
                    (
                        "Werkzeuge",
                        vec![
                            M::I("Ungültige Werte", "", true),
                            M::I("Konfliktbericht", "", true),
                            M::I("Einschränkungsdefinition aktualisieren", "", true),
                            M::I("Bedingungen verifizieren", "", false),
                        ],
                    ),
                    (
                        "Anzeigen",
                        vec![
                            M::I("Verwendungsnachweis", "", true),
                            M::I("Gesteuert durch...", "", true),
                            M::I("Steuernd...", "", true),
                            M::I("Einschränkungstabellen...", "", false),
                            M::Sep,
                            M::I("Informationen...", "", true),
                        ],
                    ),
                ],
            );
            pad(ui, 10.0, |ui| {
                search_in(ui, "par", &mut t.par.scope, &SCOPES, &model);
                ui.add_space(4.0);
                hline(ui);
                egui::Frame::NONE.stroke(Stroke::new(1.0_f32, Color32::from_rgb(214, 218, 222))).inner_margin(6.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if !t.par.hide_filter {
                        ui.horizontal(|ui| {
                            lbl(ui, "Filtern nach");
                            combo(ui, "par_filt", ui.available_width() - 108.0, &mut t.par.filt, &filters, true, None);
                            if btn(ui, "Anpassen...", 96.0, 0) {
                                t.par.filter = true;
                            }
                        });
                    }
                    param_table(ui, &mut t.par, 290.0);
                    param_footer(ui, &mut t.par, "par");
                });
            });
            (ok, cancel, reset) = bottom_buttons(ui, true);
        });
        if let Some(h) = hit {
            let rel = self.relations_draft.clone().unwrap_or_else(|| self.data.relations.clone());
            if h == "Suchen" {
                t.par.find = true;
            } else if h == "Informationen..." {
                self.param_info(t);
            } else if !self.par_menu_hit(ctx, t, h, &rel) {
                self.todo(h);
            }
        }
        if reset {
            t.par.draft = Some(self.data.parameters.clone());
            t.par.designated = self.data.designated.iter().cloned().collect();
            t.par.restricted = self.data.restricted.iter().cloned().collect();
        }
        if ok {
            self.apply_params(t, None);
        }
        if ok || cancel || !open {
            self.windows.parameters = false;
            if !self.windows.relations {
                t.par.draft = None;
            }
        }
    }

    fn param_info(&mut self, t: &mut ToolsUi) {
        let mut text =
            format!("PARAMETER FÜR TEIL {}\n================================================\n\n{:<20}{:<14}{:<18}{}\n", self.model_label(), "Name", "Typ", "Wert", "Beschreibung");
        for p in t.par.draft.as_deref().unwrap_or(&self.data.parameters) {
            let v = self.result.vars.get(&p.name.to_uppercase()).map(|v| fmt_num(*v, 4)).unwrap_or_else(|| p.expr.clone());
            text += &format!("{:<20}{:<14}{:<18}{}\n", p.name.to_uppercase(), "Reelle Zahl", v, p.note);
        }
        t.info = Some(("Informationsfenster (params.inf)".into(), text));
    }

    fn dlg_relations(&mut self, ctx: &egui::Context, t: &mut ToolsUi) {
        if !self.windows.relations {
            t.rel_hist.clear();
            t.rel_redo.clear();
            t.rel_prev = None;
            t.rel_find = None;
            t.rel_pick = None;
            return;
        }
        let model = self.model_label();
        let mut draft = self.relations_draft.clone().unwrap_or_else(|| self.data.relations.clone());
        let filters = t.par.filters();
        let filters: Vec<&str> = filters.iter().map(String::as_str).collect();
        let edit_id = egui::Id::new("rel_edit");
        // a pending selection (Alle auswählen, Suchen) goes into the editor before it is drawn
        if let Some((a, b)) = t.rel_select.take() {
            let mut st = egui::TextEdit::load_state(ctx, edit_id).unwrap_or_default();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(a), egui::text::CCursor::new(b))));
            st.store(ctx, edit_id);
            ctx.memory_mut(|m| m.request_focus(edit_id));
        }
        let (mut ok, mut cancel, mut reset, mut hit, mut verify, mut switch) = (false, false, false, None, false, false);
        let (mut undo, mut redo) = (false, false);
        let open = cwin(ctx, "dlg_rel", "Beziehungen", true, pos2(300.0, 70.0), 812.0, |ui| {
            hit = menubar(
                ui,
                "rel",
                &[
                    (
                        "Datei",
                        vec![
                            M::I("Beziehungen importieren...", "", true),
                            M::I("Beziehungen exportieren...", "", true),
                            M::Sep,
                            M::I("Konfiguration speichern als...", "", true),
                            M::I("Konfiguration laden...", "", true),
                        ],
                    ),
                    (
                        "Editieren",
                        vec![
                            M::I("Widerrufen", "Strg+Z", !t.rel_hist.is_empty()),
                            M::I("Wiederherstellen", "Strg+Y", !t.rel_redo.is_empty()),
                            M::Sep,
                            M::I("Ausschneiden", "Strg+X", false),
                            M::I("Kopieren", "Strg+C", true),
                            M::I("Einfügen", "Strg+V", false),
                            M::I("Löschen", "", true),
                            M::Sep,
                            M::I("Alle auswählen", "", true),
                            M::Sep,
                            M::I("Suchen", "Strg+F", true),
                            M::I("Suchen und Ersetzen", "", true),
                            M::I("Filter ausblenden", "", true),
                        ],
                    ),
                    ("Einfügen", vec![M::I("Funktion...", "", true), M::I("Aus Liste...", "", true)]),
                    ("Parameter", par_menu()),
                    (
                        "Dienstprogramme",
                        vec![
                            M::I("Auswerten", "", true),
                            M::I("Verifizieren", "", true),
                            M::I("Einheiten angeben", "", true),
                            M::Chk("Einheitsbezogen", t.rel_units, true),
                            M::I("Beziehungen umordnen", "", true),
                            M::Sep,
                            M::I("Hervorheben", "", true),
                            M::I("Bemaßungen wechseln", "", true),
                            M::Sep,
                            M::Sub("C-Programm", true, vec![M::I("C-Programm editieren", "", true), M::I("C-Programm verknüpfen", "", true)]),
                        ],
                    ),
                    (
                        "Anzeigen",
                        vec![
                            M::I("Sitzungs-ID", "", false),
                            M::I("Gesteuert durch...", "", true),
                            M::I("Steuernd...", "", true),
                            M::I("Einschränkungstabellen...", "", false),
                            M::Sep,
                            M::I("Informationen...", "", true),
                        ],
                    ),
                ],
            );
            pad(ui, 10.0, |ui| {
                if !t.rel_hide {
                    search_in(ui, "rel", &mut t.rel_scope, &["Teil", "KE", "Geerbt", "Schnitt", "Muster", "Volle Schweißnaht"], &model);
                }
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                    super::ribbon::tri(ui.painter(), r.center(), TEXT);
                    lbl(ui, "Beziehungen");
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if tool(ui, Icon::Undo, !t.rel_hist.is_empty(), "Widerrufen").clicked() {
                        undo = true;
                    }
                    if tool(ui, Icon::Redo, !t.rel_redo.is_empty(), "Wiederherstellen").clicked() {
                        redo = true;
                    }
                    tool(ui, Icon::Cut, false, "Ausschneiden");
                    if tool(ui, Icon::Copy, true, "Kopieren").clicked() {
                        hit = Some("Kopieren");
                    }
                    tool(ui, Icon::Paste, false, "Einfügen");
                    if tool(ui, Icon::Delete, true, "Gewählte Elemente löschen").clicked() {
                        hit = Some("Löschen");
                    }
                    if tool(ui, Icon::TlSwitchDims, true, "Zwischen Bemaßungswerten und -namen umschalten").clicked() {
                        switch = true;
                    }
                    if tool_text(ui, "=?", "Wert für Parameter oder Ausdruck liefern").clicked() {
                        hit = Some("Auswerten");
                    }
                    if tool(ui, Icon::DwgDim, true, "Bemaßungsnamen im Modell anzeigen").clicked() {
                        switch = true;
                    }
                    if tool(ui, Icon::Search, true, "Suchen").clicked() {
                        hit = Some("Suchen");
                    }
                    if tool_text(ui, "ƒx", "Funktionen aus Liste einfügen").clicked() {
                        hit = Some("Funktion...");
                    }
                    if tool(ui, Icon::TlParams, true, "Parameternamen aus Liste einfügen").clicked() {
                        hit = Some("Aus Liste...");
                    }
                    if tool(ui, Icon::TlHistory, true, "Einheiten für Beziehungen angeben").clicked() {
                        hit = Some("Einheiten angeben");
                    }
                    if tool(ui, Icon::TlColumns, true, "Beziehungen umordnen").clicked() {
                        hit = Some("Beziehungen umordnen");
                    }
                    if tool(ui, Icon::TlVerify, true, "Beziehungen verifizieren").clicked() {
                        verify = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for op in ["+", "-", "*", "/", "^", "()", "[]", "="] {
                            let (r, resp) = ui.allocate_exact_size(vec2(24.0, 25.0), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(r, 2.0, HOVER_BG);
                            }
                            ui.painter().text(
                                r.center(),
                                Align2::CENTER_CENTER,
                                match op {
                                    "*" => "×",
                                    "-" => "−",
                                    o => o,
                                },
                                egui::FontId::monospace(13.0),
                                TEXT,
                            );
                            if resp.clicked() {
                                draft.push_str(op);
                            }
                        }
                    });
                    egui::ScrollArea::vertical().id_salt("rel_text").max_height(if t.rel_local { 220.0 } else { 380.0 }).show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut draft)
                                .id(edit_id)
                                .font(font())
                                .desired_rows(if t.rel_local { 11 } else { 19 })
                                .desired_width(f32::INFINITY)
                                .margin(vec2(8.0, 8.0)),
                        );
                    });
                });
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        combo(ui, "rel_order", 170.0, &mut t.rel_order, &["Ursprünglich", "Nach Regenerierung"], true, None);
                    });
                });
                hline(ui);
                let head = ui
                    .horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                        let c = r.center();
                        let pts = if t.rel_local {
                            vec![c + vec2(-5.0, -3.0), c + vec2(5.0, -3.0), c + vec2(0.0, 4.0)]
                        } else {
                            vec![c + vec2(-3.0, -5.0), c + vec2(4.0, 0.0), c + vec2(-3.0, 5.0)]
                        };
                        ui.painter().add(egui::Shape::convex_polygon(pts, TEXT, Stroke::NONE));
                        lbl(ui, "Lokale Parameter");
                    })
                    .response
                    .interact(Sense::click());
                if head.clicked() {
                    t.rel_local = !t.rel_local;
                }
                if t.rel_local {
                    egui::Frame::NONE.stroke(Stroke::new(1.0_f32, Color32::from_rgb(214, 218, 222))).inner_margin(6.0).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            lbl(ui, "Filtern nach");
                            combo(ui, "rel_filt", 300.0, &mut t.par.filt, &filters, true, None);
                            lbl(ui, "Unterelemente");
                            let mut z = 0;
                            combo(ui, "rel_sub", ui.available_width(), &mut z, &[""], true, None);
                        });
                        param_table(ui, &mut t.par, 110.0);
                        param_footer(ui, &mut t.par, "rel");
                    });
                }
            });
            (ok, cancel, reset) = bottom_buttons(ui, true);
        });
        // Suchen / Suchen und Ersetzen
        if let Some(f) = t.rel_find.as_mut() {
            let (mut close, mut next, mut rep, mut all) = (false, false, false, false);
            let o = cwin_top(ctx, "dlg_relfind", if f.replace { "Suchen und Ersetzen" } else { "Suchen" }, false, pos2(520.0, 250.0), 430.0, |ui| {
                pad(ui, 10.0, |ui| {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(96.0, 32.0), Sense::hover());
                        ui.painter().text(r.left_center(), Align2::LEFT_CENTER, "Suchen nach:", font(), TEXT);
                        field(ui, &mut f.find, ui.available_width(), true);
                    });
                    if f.replace {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(96.0, 32.0), Sense::hover());
                            ui.painter().text(r.left_center(), Align2::LEFT_CENTER, "Ersetzen durch:", font(), TEXT);
                            field(ui, &mut f.repl, ui.available_width(), true);
                        });
                    }
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if btn(ui, "Schließen", 84.0, 0) {
                                close = true;
                            }
                            if f.replace {
                                if btn(ui, "Alle ersetzen", 100.0, 0) {
                                    all = true;
                                }
                                if btn(ui, "Ersetzen", 84.0, 0) {
                                    rep = true;
                                }
                            }
                            if btn(ui, "Weitersuchen", 100.0, 1) {
                                next = true;
                            }
                        });
                    });
                });
            });
            let lower = draft.to_lowercase();
            let needle = f.find.to_lowercase();
            let chars = |b: usize| draft[..b].chars().count();
            if !needle.is_empty() {
                if all {
                    let n = lower.matches(&needle).count();
                    if n > 0 {
                        draft = replace_ci(&draft, &f.find, &f.repl);
                    }
                    self.say(format!("{} Vorkommen ersetzt.", n), MsgKind::Info);
                } else if next || rep {
                    let from = f.pos.min(lower.len());
                    let at = lower[from..].find(&needle).map(|i| i + from).or_else(|| lower.find(&needle));
                    match at {
                        Some(a) if rep => {
                            draft.replace_range(a..a + needle.len(), &f.repl);
                            f.pos = a + f.repl.len();
                            t.rel_select = Some((draft[..a].chars().count(), draft[..a + f.repl.len()].chars().count()));
                        }
                        Some(a) => {
                            f.pos = a + needle.len();
                            t.rel_select = Some((chars(a), chars(a + needle.len())));
                        }
                        None => self.say(format!("„{}“ wurde nicht gefunden.", f.find), MsgKind::Warning),
                    }
                }
            }
            if close || !o {
                t.rel_find = None;
            }
        }
        // Einfügen ▸ Funktion… / Aus Liste…
        if let Some(kind) = t.rel_pick {
            let names: Vec<(String, String)> = if kind == 0 {
                FUNCS.iter().map(|f| (f.0.to_string(), f.1.to_string())).collect()
            } else {
                let mut v: Vec<(String, String)> = t.par.draft.as_deref().unwrap_or(&self.data.parameters).iter().map(|p| (p.name.to_uppercase(), p.note.clone())).collect();
                for l in rel_lines(&draft) {
                    if !v.iter().any(|x| x.0 == l.0) {
                        v.push((l.0, "Durch Beziehung definiert".into()));
                    }
                }
                v
            };
            let (mut close, mut insert) = (false, false);
            let o = cwin_top(ctx, "dlg_relpick", if kind == 0 { "Funktion einfügen" } else { "Aus Liste auswählen" }, false, pos2(560.0, 200.0), 400.0, |ui| {
                pad(ui, 10.0, |ui| {
                    egui::Frame::NONE.fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, LINE)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 0.0;
                        egui::ScrollArea::vertical().id_salt("relpick").auto_shrink([false, false]).max_height(300.0).show(ui, |ui| {
                            for (i, (n, d)) in names.iter().enumerate() {
                                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                                if t.rel_pick_sel == i {
                                    ui.painter().rect_filled(r, 0.0, ROW_SEL);
                                } else if resp.hovered() {
                                    ui.painter().rect_filled(r, 0.0, HOVER_BG);
                                }
                                ui.painter().text(r.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, n, font(), TEXT);
                                ui.painter().with_clip_rect(r).text(r.left_center() + vec2(120.0, 0.0), Align2::LEFT_CENTER, d, font(), Color32::from_rgb(120, 124, 130));
                                if resp.clicked() {
                                    t.rel_pick_sel = i;
                                }
                                if resp.double_clicked() {
                                    t.rel_pick_sel = i;
                                    insert = true;
                                }
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if btn(ui, "Schließen", 84.0, 0) {
                                close = true;
                            }
                            if btn(ui, "Einfügen", 84.0, 1) {
                                insert = true;
                            }
                        });
                    });
                });
            });
            if insert {
                if let Some((n, _)) = names.get(t.rel_pick_sel) {
                    draft.push_str(n);
                }
            }
            if close || !o {
                t.rel_pick = None;
                t.rel_pick_sel = 0;
            }
        }
        if let Some(h) = hit {
            match h {
                "Verifizieren" => verify = true,
                "Auswerten" => {
                    let mut d = self.data.clone();
                    d.relations = draft.clone();
                    if let Some(p) = &t.par.draft { d.parameters = p.clone(); }
                    let (vars, errs) = d.evaluate_parameters();
                    let mut keys: Vec<_> = vars.iter().collect();
                    keys.sort_by(|a, b| a.0.cmp(b.0));
                    let mut text = format!("AUSWERTUNG DER BEZIEHUNGEN FÜR TEIL {}\n================================================\n\n", model);
                    for (k, v) in keys { text += &format!("{:<24}= {}\n", k, fmt_num(*v, 6)); }
                    for e in errs { text += &format!("\nFEHLER: {}", e); }
                    t.info = Some(("Informationsfenster (rel.ptd)".into(), text));
                }
                "Bemaßungen wechseln" => switch = true,
                "Einheitsbezogen" => t.rel_units = !t.rel_units,
                "Einheiten angeben" => t.info = Some(("Informationsfenster (einheiten.inf)".into(), format!("EINHEITEN FÜR TEIL {}\n================================================\n\nEinheitensystem:  Millimeter Kilogramm Sekunde (mmKs)\nLänge:            mm\nMasse:            kg\nZeit:             s\nWinkel:           Grad\n\nBeziehungen werden {}ausgewertet.\n", model, if t.rel_units { "einheitsbezogen " } else { "ohne Einheiten " }))),
                "Beziehungen umordnen" => { let n = reorder_relations(&draft); if n == draft { self.say("Die Beziehungen stehen bereits in der Reihenfolge ihrer Auswertung.", MsgKind::Info); } else { draft = n; self.say("Beziehungen wurden in die Reihenfolge ihrer Auswertung gebracht.", MsgKind::Info); } }
                "Hervorheben" => {
                    let vars: Vec<String> = rel_lines(&draft).into_iter().map(|l| l.0).collect();
                    let ids: Vec<u64> = self.data.features.iter().filter(|f| serde_json::to_string(&f.kind).map(|j| vars.iter().any(|v| has_word(&j, v))).unwrap_or(false)).map(|f| f.id).collect();
                    self.say(format!("{} Konstruktionselement(e) verwenden Werte aus den Beziehungen.", ids.len()), MsgKind::Info);
                    self.tree_selection = ids;
                }
                "C-Programm editieren" | "C-Programm verknüpfen" => self.say("Für dieses Modell ist kein C-Programm vorhanden.", MsgKind::Warning),
                "Widerrufen" => undo = true,
                "Wiederherstellen" => redo = true,
                "Kopieren" => { ctx.copy_text(draft.clone()); self.say("Beziehungen in die Zwischenablage kopiert.", MsgKind::Info); }
                "Alle auswählen" => t.rel_select = Some((0, draft.chars().count())),
                "Löschen" => draft.clear(),
                "Suchen" => t.rel_find = Some(RelFind::default()),
                "Suchen und Ersetzen" => t.rel_find = Some(RelFind { replace: true, ..Default::default() }),
                "Filter ausblenden" => t.rel_hide = !t.rel_hide,
                "Funktion..." => { t.rel_pick = Some(0); t.rel_pick_sel = 0; }
                "Aus Liste..." => { t.rel_pick = Some(1); t.rel_pick_sel = 0; }
                "Beziehungen exportieren..." => { let name = format!("{}_beziehungen.txt", self.data.name.to_lowercase()); self.save_text("Beziehungen exportieren", &name, &draft); }
                "Beziehungen importieren..." => { if let Some(s) = self.load_text("Beziehungen importieren") { if !draft.is_empty() && !draft.ends_with('\n') { draft.push('\n'); } draft.push_str(s.trim_end()); self.say("Beziehungen importiert.", MsgKind::Info); } }
                "Informationen..." => t.info = Some(("Informationsfenster (rel.inf)".into(), format!("BEZIEHUNGEN FÜR TEIL {}\n================================================\n\n{}\n", model, draft))),
                h => { let rel = draft.clone(); if !self.par_menu_hit(ctx, t, h, &rel) { self.todo(h); } else if h.starts_with("Parameter") || h == "Aufheben" || h.starts_with("Nach ") { t.rel_local = true; } }
            }
        }
        if switch {
            t.dim_names = !t.dim_names;
            self.say(if t.dim_names { "Bemaßungen werden als Namen angezeigt." } else { "Bemaßungen werden als Werte angezeigt." }, MsgKind::Info);
        }
        if verify {
            let mut d = self.data.clone();
            d.relations = draft.clone();
            if let Some(p) = &t.par.draft {
                d.parameters = p.clone();
            }
            let (_, errs) = d.evaluate_parameters();
            if errs.is_empty() {
                self.say("Beziehungen wurden erfolgreich verifiziert.", MsgKind::Success);
            } else {
                for e in errs {
                    self.say(e, MsgKind::Error);
                }
            }
        }
        if reset {
            draft = self.data.relations.clone();
            t.par.draft = Some(self.data.parameters.clone());
        }
        // Widerrufen / Wiederherstellen work on snapshots of the text
        let prev = t.rel_prev.clone().unwrap_or_else(|| draft.clone());
        if undo {
            if let Some(s) = t.rel_hist.pop() {
                t.rel_redo.push(draft.clone());
                draft = s;
            }
        } else if redo {
            if let Some(s) = t.rel_redo.pop() {
                t.rel_hist.push(draft.clone());
                draft = s;
            }
        } else if draft != prev {
            t.rel_hist.push(prev);
            t.rel_redo.clear();
            if t.rel_hist.len() > 200 {
                t.rel_hist.remove(0);
            }
        }
        t.rel_prev = Some(draft.clone());
        if ok {
            self.apply_params(t, Some(draft.clone()));
        }
        if ok || cancel || !open {
            self.windows.relations = false;
            self.relations_draft = None;
            if !self.windows.parameters {
                t.par.draft = None;
            }
        } else {
            self.relations_draft = Some(draft);
        }
    }

    /// Parametereigenschaften, Parametertabellenspalten, Suchen and Filterparameter.
    fn dlg_par_sub(&mut self, ctx: &egui::Context, t: &mut ToolsUi) {
        let p = &mut t.par;
        let mut say: Option<String> = None;
        if let (Some(mut tab), Some(list)) = (p.props, p.draft.as_mut()) {
            let mut close = false;
            if let Some(cur) = list.get_mut(p.sel) {
                let key = cur.name.to_uppercase();
                let open = cwin_top(ctx, "dlg_parprops", "Parametereigenschaften", false, pos2(520.0, 110.0), 480.0, |ui| {
                    pad(ui, 10.0, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        tabs(ui, &mut tab, &["Allgemein", "Liste/Reihe", "Definition"], 9);
                        tab_page(ui, |ui| {
                            ui.set_min_height(620.0);
                            match tab {
                                0 => {
                                    fieldset(ui, "Name", |ui| {
                                        ui.horizontal(|ui| {
                                            field(ui, &mut cur.name, 280.0, true);
                                            let mut r = p.restricted.contains(&key);
                                            if chk(ui, &mut r, "Eingeschränkt", true) {
                                                if r {
                                                    p.restricted.insert(key.clone());
                                                } else {
                                                    p.restricted.remove(&key);
                                                }
                                            }
                                        });
                                    });
                                    let mut z = 0;
                                    fieldset(ui, "Typ", |ui| {
                                        combo(ui, "pp_typ", ui.available_width(), &mut z, &["Reelle Zahl"], false, None);
                                    });
                                    fieldset(ui, "Wert", |ui| {
                                        ui.horizontal(|ui| {
                                            field(ui, &mut cur.expr, 300.0, true);
                                            let mut f = false;
                                            chk(ui, &mut f, "Liste/Reihe", false);
                                        });
                                    });
                                    fieldset(ui, "Einheitenmenge", |ui| {
                                        combo(ui, "pp_um", ui.available_width(), &mut z, &[""], false, None);
                                    });
                                    fieldset(ui, "Einheiten", |ui| {
                                        combo(ui, "pp_u", ui.available_width(), &mut z, &[""], false, None);
                                    });
                                    fieldset(ui, "Optionen", |ui| {
                                        let mut d = p.designated.contains(&key);
                                        if chk(ui, &mut d, "Ausgewiesen", true) {
                                            if d {
                                                p.designated.insert(key.clone());
                                            } else {
                                                p.designated.remove(&key);
                                            }
                                        }
                                    });
                                    fieldset(ui, "Zugriff", |ui| {
                                        combo(ui, "pp_acc", ui.available_width(), &mut z, &["Voll"], true, Some(Icon::TlLock));
                                    });
                                    fieldset(ui, "Quelle", |ui| {
                                        readout(ui, "Benutzerdefiniert", ui.available_width());
                                    });
                                    fieldset(ui, "Besitzertabelle", |_| {});
                                    fieldset(ui, "Beschreibung", |ui| {
                                        field(ui, &mut cur.note, ui.available_width(), true);
                                    });
                                }
                                1 => {
                                    radio(ui, true, "Liste", false);
                                    radio(ui, false, "Reihe", false);
                                    ui.add_space(8.0);
                                    lbl(ui, "Parametereinheiten");
                                    let mut z = 0;
                                    ui.horizontal(|ui| {
                                        let w = (ui.available_width() - 6.0) / 2.0;
                                        combo(ui, "pp_e1", w, &mut z, &[""], false, None);
                                        combo(ui, "pp_e2", w, &mut z, &[""], false, None);
                                    });
                                    lbl(ui, "Werte");
                                    ui.horizontal(|ui| {
                                        white_box(ui, ui.available_width() - 48.0, 440.0, Color32::WHITE);
                                        ui.vertical(|ui| {
                                            ui.add_space(200.0);
                                            ibox(ui, Icon::TlAddRow, false, "");
                                            ibox(ui, Icon::TlAddRow, false, "");
                                        });
                                    });
                                }
                                _ => {
                                    fieldset(ui, "Basistyp", |ui| {
                                        readout(ui, "Reelle Zahl", ui.available_width());
                                    });
                                    fieldset(ui, "Eingeschränkte Optionen", |ui| {
                                        radio(ui, false, "Bereich", false);
                                        let mut z = 0;
                                        let mut s = String::new();
                                        ui.horizontal(|ui| {
                                            combo(ui, "pp_gt", 180.0, &mut z, &["Größer als"], false, None);
                                            field(ui, &mut s, ui.available_width(), false);
                                        });
                                        ui.horizontal(|ui| {
                                            combo(ui, "pp_lt", 180.0, &mut z, &["Kleiner als"], false, None);
                                            field(ui, &mut s, ui.available_width(), false);
                                        });
                                        radio(ui, true, "Liste", true);
                                        lbl(ui, "Werte");
                                        ui.horizontal(|ui| {
                                            let (r, _) = white_box(ui, ui.available_width() - 48.0, 360.0, Color32::WHITE);
                                            ui.painter().text(r.left_top() + vec2(10.0, 14.0), Align2::LEFT_CENTER, "Werte", font(), TEXT);
                                            ui.painter().line_segment([r.left_top() + vec2(1.0, 28.0), r.right_top() + vec2(-1.0, 28.0)], Stroke::new(1.0_f32, LINE));
                                            // the value the parameter has now is the one entry of its list
                                            ui.painter().text(r.left_top() + vec2(10.0, 44.0), Align2::LEFT_CENTER, &cur.expr, font(), TEXT);
                                            ui.vertical(|ui| {
                                                ui.add_space(150.0);
                                                ibox(ui, Icon::TlAddRow, false, "");
                                                ibox(ui, Icon::TlAddRow, false, "");
                                            });
                                        });
                                    });
                                }
                            }
                        });
                    });
                    hline(ui);
                    pad(ui, 10.0, |ui| {
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if btn(ui, "Abbrechen", 100.0, 0) {
                                    close = true;
                                }
                                if btn(ui, "OK", 100.0, 1) {
                                    close = true;
                                }
                            });
                        });
                    });
                });
                if !open {
                    close = true;
                }
            } else {
                close = true;
            }
            p.props = if close { None } else { Some(tab) };
        }
        if p.cols {
            let mut close = false;
            let mut shown = p.shown();
            let hidden: Vec<usize> = (0..COLS.len()).filter(|c| !shown.contains(c)).collect();
            let open = cwin_top(ctx, "dlg_parcols", "Parametertabellenspalten", false, pos2(380.0, 210.0), 713.0, |ui| {
                pad(ui, 10.0, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(210.0);
                            fieldset(ui, "Nicht angezeigt", |ui| {
                                egui::Frame::NONE.fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, LINE)).show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.set_min_height(300.0);
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    for (i, c) in hidden.iter().enumerate() {
                                        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                                        if p.hid_sel == i {
                                            ui.painter().rect_filled(r, 0.0, ROW_SEL);
                                        }
                                        ui.painter().text(r.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, COLS[*c], font(), TEXT);
                                        if resp.clicked() {
                                            p.hid_sel = i;
                                        }
                                    }
                                });
                            });
                        });
                        ui.vertical(|ui| {
                            ui.add_space(130.0);
                            if btn(ui, ">>", 46.0, if hidden.is_empty() { 2 } else { 0 }) {
                                if let Some(c) = hidden.get(p.hid_sel.min(hidden.len().saturating_sub(1))) {
                                    shown.push(*c);
                                    p.hid_sel = 0;
                                }
                            }
                            ui.add_space(6.0);
                            let sel = p.col_sel.max(1).min(shown.len().saturating_sub(1));
                            if btn(ui, "<<", 46.0, if shown.len() > 1 { 0 } else { 2 }) && sel >= 1 && sel < shown.len() {
                                shown.remove(sel);
                            }
                        });
                        ui.vertical(|ui| {
                            ui.set_width(ui.available_width());
                            fieldset(ui, "Angezeigt", |ui| {
                                egui::Frame::NONE.fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, LINE)).show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    let row = |ui: &mut egui::Ui, a: &str, b: &str, c: &str, sel: bool, dim: bool, head: bool| -> bool {
                                        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                                        if head {
                                            ui.painter().rect_filled(r, 0.0, Color32::from_rgb(246, 247, 248));
                                            ui.painter().line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0_f32, LINE));
                                        }
                                        if sel {
                                            ui.painter().rect_filled(r, 0.0, ROW_SEL);
                                        }
                                        let col = if dim { DIM_TEXT } else { TEXT };
                                        for (x, s) in [(if head { 10.0 } else { 18.0 }, a), (if head { 72.0 } else { 76.0 }, b), (if head { 212.0 } else { 220.0 }, c)] {
                                            ui.painter().text(pos2(r.left() + x, r.center().y), Align2::LEFT_CENTER, s, font(), col);
                                        }
                                        resp.clicked()
                                    };
                                    row(ui, "Reihenf.", "Spaltenname", "Breite", false, false, true);
                                    egui::ScrollArea::vertical().id_salt("parcols").auto_shrink([false, false]).max_height(236.0).show(ui, |ui| {
                                        for (i, c) in shown.iter().enumerate() {
                                            if row(ui, &(i + 1).to_string(), COLS[*c], "5", p.col_sel.max(1) == i, i == 0, false) && i > 0 {
                                                p.col_sel = i;
                                            }
                                        }
                                    });
                                });
                                ui.horizontal(|ui| {
                                    let sel = p.col_sel.max(1);
                                    if ibox(ui, Icon::TlUp, sel > 1 && sel < shown.len(), "").clicked() {
                                        shown.swap(sel, sel - 1);
                                        p.col_sel = sel - 1;
                                    }
                                    if ibox(ui, Icon::TlDown, sel + 1 < shown.len(), "").clicked() {
                                        shown.swap(sel, sel + 1);
                                        p.col_sel = sel + 1;
                                    }
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let mut s = "5".to_string();
                                        field(ui, &mut s, 70.0, false);
                                        lbl(ui, "Breite:");
                                    });
                                });
                            });
                        });
                    });
                });
                hline(ui);
                pad(ui, 10.0, |ui| {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if btn(ui, "Abbrechen", 96.0, 0) {
                                close = true;
                            }
                            if btn(ui, "OK", 96.0, 1) {
                                close = true;
                            }
                            btn(ui, "Anwenden", 96.0, 0);
                        });
                    });
                });
            });
            p.cols_shown = if shown.len() == COLS.len() && shown.iter().enumerate().all(|(i, c)| i == *c) { vec![] } else { shown };
            if close || !open {
                p.cols = false;
            }
        }
        if p.find {
            let (mut close, mut go) = (false, false);
            let open = cwin_top(ctx, "dlg_parfind", "Suchen", false, pos2(480.0, 300.0), 650.0, |ui| {
                pad(ui, 6.0, |ui| {
                    lbl(ui, "Elemente suchen, die folgenden Regeln entsprechen:");
                    egui::Frame::NONE.fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, LINE)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.set_height(130.0);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        egui::ScrollArea::vertical().id_salt("parrules").auto_shrink([false, false]).show(ui, |ui| {
                            for (i, r) in p.rules.iter().enumerate() {
                                let (rr, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                                if p.rule_sel == Some(i) {
                                    ui.painter().rect_filled(rr, 0.0, ROW_SEL);
                                }
                                let join = if i == 0 {
                                    ""
                                } else if r.or {
                                    "ODER "
                                } else {
                                    "UND "
                                };
                                ui.painter().text(
                                    rr.left_center() + vec2(8.0, 0.0),
                                    Align2::LEFT_CENTER,
                                    format!("{}{} {} {}", join, COLS[r.col], ["=", "!=", "<", ">"][r.cmp], r.val),
                                    font(),
                                    TEXT,
                                );
                                if resp.clicked() {
                                    p.rule_sel = Some(i);
                                }
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let has = p.rule_sel.is_some_and(|i| i < p.rules.len());
                            if btn(ui, "Entfernen", 86.0, if has { 0 } else { 2 }) {
                                if let Some(i) = p.rule_sel.take() {
                                    p.rules.remove(i);
                                }
                            }
                            if btn(ui, "Ändern", 80.0, if has { 0 } else { 2 }) {
                                if let Some(i) = p.rule_sel.take() {
                                    let r = p.rules.remove(i);
                                    p.f_or = if r.or { 0 } else { 1 };
                                    p.f_col = r.col;
                                    p.f_cmp = r.cmp;
                                    p.f_val = r.val;
                                }
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        lbl(ui, "Regeldefinition");
                        hline(ui);
                    });
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            lbl(ui, "Regel hinzufügen als:");
                            combo(ui, "pf_a", 140.0, &mut p.f_or, &["ODER", "UND"], true, None);
                        });
                        ui.vertical(|ui| {
                            lbl(ui, "Spalte:");
                            combo(ui, "pf_b", 150.0, &mut p.f_col, &COLS, true, None);
                        });
                        ui.vertical(|ui| {
                            lbl(ui, "Bedingung:");
                            combo(ui, "pf_c", 78.0, &mut p.f_cmp, &["=", "!=", "<", ">"], true, None);
                        });
                        ui.vertical(|ui| {
                            lbl(ui, "Wert:");
                            ui.horizontal(|ui| {
                                field(ui, &mut p.f_val, 120.0, true);
                                if btn(ui, "Hinzufügen", 90.0, 0) && !p.f_val.trim().is_empty() {
                                    p.rules.push(Rule { or: p.f_or == 0, col: p.f_col, cmp: p.f_cmp, val: p.f_val.trim().to_string() });
                                    p.f_val.clear();
                                }
                            });
                        });
                    });
                    hline(ui);
                    ui.horizontal(|ui| {
                        ui.add_space(110.0);
                        if btn(ui, "Suchen", 90.0, 0) {
                            go = true;
                        }
                        ui.add_space(220.0);
                        if btn(ui, "Schließen", 90.0, 0) {
                            close = true;
                        }
                    });
                });
            });
            if go {
                if p.rules.is_empty() {
                    say = Some("Zuerst eine Regel mit „Hinzufügen“ definieren.".into());
                } else if let Some(list) = p.draft.as_ref() {
                    let hits: Vec<usize> = (0..list.len()).filter(|i| rules_match(p, &list[*i])).collect();
                    // the next hit after the selected row
                    if let Some(n) = hits.iter().copied().find(|i| *i > p.sel).or(hits.first().copied()) {
                        p.sel = n;
                    }
                    say = Some(format!("{} Parameter entsprechen den Regeln.", hits.len()));
                }
            }
            if close || !open {
                p.find = false;
            }
        }
        if p.filter {
            let mut close = false;
            let filters = p.filters();
            let open = cwin_top(ctx, "dlg_parfilter", "Filterparameter", false, pos2(330.0, 170.0), 860.0, |ui| {
                pad(ui, 10.0, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(300.0);
                            fieldset(ui, "Gespeicherte Filter", |ui| {
                                egui::Frame::NONE.fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, LINE)).show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.set_min_height(300.0);
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    for (i, f) in filters.iter().enumerate() {
                                        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
                                        if p.filt == i {
                                            ui.painter().rect_filled(Rect::from_min_max(pos2(r.left() + 22.0, r.top()), r.max), 0.0, ROW_SEL);
                                            icons::paint(ui.painter(), Rect::from_center_size(pos2(r.left() + 11.0, r.center().y), vec2(12.0, 12.0)), Icon::TlPromptArrow, true);
                                        }
                                        ui.painter().with_clip_rect(r).text(pos2(r.left() + 28.0, r.center().y), Align2::LEFT_CENTER, f, font(), TEXT);
                                        if resp.clicked() {
                                            p.filt = i;
                                        }
                                    }
                                });
                                if btn(ui, "Neu", 50.0, 0) {
                                    p.extra_filters.push(format!("Filter {}", p.extra_filters.len() + 1));
                                    p.filt = filters.len();
                                }
                            });
                        });
                        ui.vertical(|ui| {
                            ui.set_width(ui.available_width());
                            fieldset(ui, "Filterdefinition", |ui| {
                                lbl(ui, "Umfangssteuerung:");
                                radio(ui, true, "In aktuellen Kontextebenen filtern", false);
                                radio(ui, false, "In aktuellen und allen Unterebenen filtern", false);
                                let mut f = false;
                                chk(ui, &mut f, "In allen Objektfiltern anzeigen", false);
                                lbl(ui, "Elemente suchen, die folgenden Regeln entsprechen:");
                                white_box(ui, ui.available_width(), 110.0, Color32::WHITE);
                                ui.horizontal(|ui| {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        btn(ui, "Entfernen", 80.0, 2);
                                        btn(ui, "Ändern", 70.0, 2);
                                    });
                                });
                                ui.horizontal(|ui| {
                                    lbl(ui, "Regeldefinition");
                                    hline(ui);
                                });
                                let mut z = 0;
                                let mut s = String::new();
                                ui.horizontal(|ui| {
                                    lbl(ui, "Regel hinzufügen als:");
                                    combo(ui, "pfl_a", 96.0, &mut z, &["ODER"], false, None);
                                    combo(ui, "pfl_b", 110.0, &mut z, &["Besitzer"], false, None);
                                    combo(ui, "pfl_c", 54.0, &mut z, &["="], false, None);
                                    field(ui, &mut s, 90.0, false);
                                    btn(ui, "Hinzufügen", 84.0, 2);
                                });
                            });
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if btn(ui, "Abbrechen", 100.0, 0) {
                                close = true;
                            }
                            if btn(ui, "OK", 100.0, 1) {
                                close = true;
                            }
                            if btn(ui, "Zurücksetzen", 100.0, 0) {
                                p.filt = 0;
                                p.extra_filters.clear();
                            }
                        });
                    });
                });
            });
            if close || !open {
                p.filter = false;
            }
        }
        if let Some(s) = say {
            self.say(s, MsgKind::Info);
        }
    }

    fn dlg_family(&mut self, ctx: &egui::Context, t: &mut ToolsUi) {
        if !self.windows.family {
            return;
        }
        let model = self.model_label();
        let empty = self.data.family.columns.is_empty() && self.data.family.instances.is_empty();
        let (mut close, mut hit) = (false, None);
        let dis = false;
        let open = cwin(ctx, "dlg_fam", &format!("Familientabelle: {}", model), true, pos2(220.0, 86.0), 990.0, |ui| {
            hit = menubar(
                ui,
                "fam",
                &[
                    (
                        "Datei",
                        vec![M::I("Tabelle importieren...", "", dis), M::Sub("Tabelle exportieren...", dis, vec![]), M::Ic(Icon::FamilyTable, "Mit Excel editieren...", "", dis)],
                    ),
                    (
                        "Editieren",
                        vec![
                            M::Ic(Icon::Cut, "Zelle ausschneiden", "Strg+X", dis),
                            M::Ic(Icon::Copy, "Zelle kopieren", "Strg+C", dis),
                            M::Ic(Icon::Paste, "Zelle einfügen", "Strg+V", dis),
                            M::Ic(Icon::Pattern, "Mit Inkrementen kopieren...", "", dis),
                            M::Sep,
                            M::Ic(Icon::TlAddRow, "Variante in generisches Modell kopieren", "", dis),
                            M::Sep,
                            M::I("Zwei Spalten vertauschen", "", dis),
                            M::I("Spalten ausblenden", "", dis),
                            M::I("Alle Spalten einblenden", "", dis),
                            M::Sep,
                            M::I("Gesamte Zeile auswählen", "Umschalt+Leertaste", dis),
                            M::I("Gesamte Spalte auswählen", "Strg+Leertaste", dis),
                            M::Sep,
                            M::I("Zeilen löschen", "", dis),
                            M::I("Spalten löschen", "", dis),
                            M::I("Gesamte Tabelle löschen", "", dis),
                            M::Sep,
                            M::Ic(Icon::TlBinoculars, "Suchen...", "Strg+F", dis),
                            M::I("Am Bildschirm hervorheben", "", dis),
                        ],
                    ),
                    (
                        "Einfügen",
                        vec![
                            M::Ic(Icon::TlAddRow, "Variantenzeile", "", true),
                            M::Ic(Icon::TlAddCol, "Spalten...", "", true),
                            M::Ic(Icon::Note, "Kommentarzeile", "", true),
                            M::Ic(Icon::FolderItem, "Tabelle auf Variantenebene", "", dis),
                        ],
                    ),
                    (
                        "Tools",
                        vec![
                            M::Ic(Icon::TlVerify, "Verifizieren...", "", dis),
                            M::Ic(Icon::TlGlasses, "Vorschau", "", dis),
                            M::Ic(Icon::TlLock, "Variante sperren/entsperren", "", dis),
                            M::I("Absolute/relative Bemaßungswerte", "", dis),
                            M::I("Externe Referenzen auf Varianten umschalten", "", dis),
                            M::I("Baugruppenkomponenten konfigurieren...", "", dis),
                            M::Sub("Ersetzen durch", dis, vec![]),
                            M::Sub("Sortieren", dis, vec![]),
                            M::Chk("System-Masseneigenschaften anzeigen", t.fam_sysmass, true),
                        ],
                    ),
                ],
            );
            pad(ui, 10.0, |ui| {
                ui.horizontal(|ui| {
                    lbl(ui, "Suchen in:");
                    let mut z = 0;
                    combo(ui, "fam_in", 260.0, &mut z, &[&model], true, None);
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for ic in [Icon::Up, Icon::Cut, Icon::Copy, Icon::Paste, Icon::Pattern] {
                        tool(ui, ic, false, "");
                    }
                    if tool(ui, Icon::TlAddRow, true, "Neue Variante in der gewählten Zeile einfügen").clicked() {
                        hit = Some("Variantenzeile");
                    }
                    if tool(ui, Icon::TlAddCol, true, "Tabellenspalten hinzufügen/löschen").clicked() {
                        hit = Some("Spalten...");
                    }
                    for ic in [Icon::TlBinoculars, Icon::TlGlasses, Icon::TlLock, Icon::TlVerify, Icon::FamilyTable] {
                        tool(ui, ic, false, "");
                    }
                });
                egui::Frame::NONE.fill(Color32::from_rgb(244, 245, 246)).stroke(Stroke::new(1.0_f32, LINE)).inner_margin(8.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_min_height(430.0);
                    if empty && !t.fam_edit {
                        lbl_bold(ui, "Modell hat z.Z. keine Familientabellen-Konstruktionsvariationen.");
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                            icons::paint(ui.painter(), r, Icon::TlAddCol, true);
                            lbl_bold(ui, "auswählen, um Bemaßungs- oder KE-Parameter-Spalten zur Tabelle hinzuzufügen;");
                        });
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                            icons::paint(ui.painter(), r, Icon::TlAddRow, true);
                            lbl_bold(ui, "auswählen, um Variantenmodell-Zeilen als Konstruktionsvariationen des aktuellen Modells");
                        });
                        lbl_bold(ui, "zu definieren und dabei für diese Parameter in jeder Variante verschiedene Werte in den Zellen einzusetzen.");
                    } else {
                        egui::ScrollArea::both().id_salt("fam_body").max_height(420.0).show(ui, |ui| {
                            ui.visuals_mut().override_text_color = None;
                            self.family_ui(ui);
                        });
                    }
                });
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(96.0, 34.0), Sense::hover());
                    ui.painter().rect_filled(r, 1.0, Color32::from_rgb(234, 236, 238));
                    ui.painter().rect_stroke(r, 1.0, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
                    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.left() + 20.0, r.center().y), vec2(18.0, 18.0)), Icon::Open, false);
                    ui.painter().text(pos2(r.left() + 38.0, r.center().y), Align2::LEFT_CENTER, "Öffnen", font(), DIM_TEXT);
                });
            });
            let (ok, cancel, _) = bottom_buttons(ui, false);
            close = ok || cancel;
        });
        match hit {
            Some("Variantenzeile") | Some("Spalten...") | Some("Kommentarzeile") => t.fam_edit = true,
            Some("System-Masseneigenschaften anzeigen") => t.fam_sysmass = !t.fam_sysmass,
            Some(h) => self.todo(h),
            None => {}
        }
        if close || !open {
            self.windows.family = false;
            t.fam_edit = false;
        }
    }

    fn dlg_messages(&mut self, ctx: &egui::Context, t: &mut ToolsUi) {
        if !self.windows.messages {
            return;
        }
        let (mut close, mut hit) = (false, None);
        let q = t.msg_find.clone().unwrap_or_default().to_lowercase();
        let hits = if q.is_empty() { 0 } else { self.messages.iter().filter(|m| m.text.to_lowercase().contains(&q)).count() };
        let open = cwin(ctx, "dlg_msg", "Mitteilungsprotokoll", false, pos2(520.0, 110.0), 476.0, |ui| {
            hit = menubar(
                ui,
                "msg",
                &[
                    ("Datei", vec![M::I("Speichern unter...", "", true), M::Sep, M::I("Schließen", "", true)]),
                    ("Editieren", vec![M::I("Suchen...", "", true)]),
                    ("Ansicht", vec![M::Chk("Zeilennummern", t.msg_lines, true)]),
                ],
            );
            find_bar(ui, &mut t.msg_find, hits);
            egui::Frame::NONE.fill(Color32::from_rgb(244, 245, 246)).inner_margin(4.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 0.0;
                egui::ScrollArea::both().id_salt("msg_list").max_height(600.0).min_scrolled_height(600.0).stick_to_bottom(true).show(ui, |ui| {
                    ui.set_min_height(592.0);
                    for (i, m) in self.messages.iter().enumerate() {
                        let text = if t.msg_lines { format!("{:>4}  {}", i + 1, m.text) } else { m.text.clone() };
                        let w = (tw(ui, &text, F) + 34.0).max(ui.available_width());
                        let (r, _) = ui.allocate_exact_size(vec2(w, 24.0), Sense::hover());
                        if !q.is_empty() && m.text.to_lowercase().contains(&q) {
                            ui.painter().rect_filled(r, 0.0, Color32::from_rgb(255, 240, 150));
                        }
                        let ic = match m.kind {
                            MsgKind::Warning => Icon::TlWarn,
                            MsgKind::Error => Icon::Failed,
                            MsgKind::Prompt => Icon::TlPromptArrow,
                            _ => Icon::TlBullet,
                        };
                        let s = if ic == Icon::TlBullet { 12.0 } else { 16.0 };
                        icons::paint(ui.painter(), Rect::from_center_size(pos2(r.left() + 13.0, r.center().y), vec2(s, s)), ic, true);
                        ui.painter().text(pos2(r.left() + 24.0, r.center().y), Align2::LEFT_CENTER, text, font(), TEXT);
                    }
                });
            });
            hline(ui);
            pad(ui, 8.0, |ui| {
                ui.vertical_centered(|ui| {
                    if btn(ui, "Schließen", 84.0, 1) {
                        close = true;
                    }
                });
            });
        });
        match hit {
            Some("Schließen") => close = true,
            Some("Zeilennummern") => t.msg_lines = !t.msg_lines,
            Some("Suchen...") => t.msg_find = Some(String::new()),
            Some("Speichern unter...") => {
                let s = self.messages.iter().map(|m| m.text.clone()).collect::<Vec<_>>().join("\n");
                self.save_text("Mitteilungsprotokoll speichern", "mitteilungen.log", &s);
            }
            Some(h) => self.todo(h),
            None => {}
        }
        if close || !open {
            self.windows.messages = false;
            t.msg_find = None;
        }
    }

    fn dlg_player(&mut self, ctx: &egui::Context, t: &mut ToolsUi) {
        if !self.creo.player {
            return;
        }
        let n = self.data.features.len();
        let first = 4.min(n).max(1);
        let mut pos = self.data.insert_index.unwrap_or(n).max(1).min(n.max(1));
        let before = pos;
        let name = |pos: usize| self.data.features.get(pos.wrapping_sub(1)).map(|f| f.name.to_uppercase().replace(' ', "_")).unwrap_or_default();
        if t.player.num_for != pos {
            t.player.num = pos.to_string();
            t.player.num_for = pos;
        }
        let (mut close, mut finish, mut info, mut dims) = (false, false, false, false);
        let open = cwin(ctx, "dlg_player", "Modell-Player", true, pos2(420.0, 170.0), 543.0, |ui| {
            pad(ui, 8.0, |ui| {
                ui.horizontal_top(|ui| {
                    egui::Frame::NONE.stroke(Stroke::new(1.0_f32, LINE)).inner_margin(10.0).show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(350.0);
                            ui.horizontal(|ui| {
                                ui.add_space(56.0);
                                ui.spacing_mut().item_spacing.x = 2.0;
                                // |◀ ◀ ■ ▶ ▶|
                                for k in 0..5 {
                                    let en = match k {
                                        0 | 1 => pos > first,
                                        2 => false,
                                        _ => pos < n,
                                    };
                                    let (r, resp) = ui.allocate_exact_size(vec2(46.0, 32.0), if en { Sense::click() } else { Sense::hover() });
                                    let p = ui.painter();
                                    p.rect_filled(
                                        r,
                                        1.0,
                                        if !en {
                                            Color32::from_rgb(232, 234, 236)
                                        } else if resp.hovered() {
                                            HOVER_BG
                                        } else {
                                            Color32::from_rgb(250, 250, 250)
                                        },
                                    );
                                    p.rect_stroke(r, 1.0, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
                                    let col = if en { Color32::from_rgb(40, 44, 50) } else { Color32::from_rgb(170, 174, 180) };
                                    let c = r.center();
                                    let tri_ = |c: egui::Pos2, dir: f32| {
                                        egui::Shape::convex_polygon(vec![c + vec2(-5.0 * dir, -6.0), c + vec2(5.0 * dir, 0.0), c + vec2(-5.0 * dir, 6.0)], col, Stroke::NONE)
                                    };
                                    match k {
                                        0 => {
                                            p.add(tri_(c + vec2(2.0, 0.0), -1.0));
                                            p.line_segment([c + vec2(-6.0, -6.0), c + vec2(-6.0, 6.0)], Stroke::new(2.0_f32, col));
                                        }
                                        1 => {
                                            p.add(tri_(c, -1.0));
                                        }
                                        2 => {
                                            p.rect_filled(Rect::from_center_size(c, vec2(12.0, 12.0)), 0.0, col);
                                        }
                                        3 => {
                                            p.add(tri_(c, 1.0));
                                        }
                                        _ => {
                                            p.add(tri_(c - vec2(2.0, 0.0), 1.0));
                                            p.line_segment([c + vec2(6.0, -6.0), c + vec2(6.0, 6.0)], Stroke::new(2.0_f32, col));
                                        }
                                    }
                                    if resp.clicked() {
                                        pos = match k {
                                            0 => first,
                                            1 => pos - 1,
                                            3 => pos + 1,
                                            _ => n,
                                        };
                                    }
                                }
                            });
                            let mut v = pos as f32;
                            if cslider(ui, &mut v, n.max(1) as f32, 348.0, n > 1) {
                                pos = (v.round() as usize).clamp(first, n.max(1));
                            }
                            ui.horizontal(|ui| {
                                ibox(ui, Icon::TlCursor, true, "KE auswählen, bis zu dem regeneriert wird");
                                readout(ui, &format!("#{} ({})", pos, name(pos)), ui.available_width());
                            });
                            ui.horizontal(|ui| {
                                ui.add_space(70.0);
                                lbl(ui, "KE-Nr.");
                                let r = field(ui, &mut t.player.num, 74.0, true);
                                if r.lost_focus() {
                                    if let Ok(v) = t.player.num.trim().parse::<usize>() {
                                        pos = v.clamp(first, n.max(1));
                                    }
                                }
                                lbl(ui, &format!("von {}", n));
                            });
                        });
                    });
                    ui.vertical(|ui| {
                        ui.add_space(8.0);
                        chk(ui, &mut t.player.regen, "KEs regenerieren", true);
                        chk(ui, &mut t.player.each, "Jedes KE anzeigen", true);
                    });
                });
                ui.horizontal(|ui| {
                    if btn(ui, "Bemaßungen anzeigen", 150.0, 0) {
                        dims = true;
                    }
                    if btn(ui, "KE-Info", 70.0, 0) {
                        info = true;
                    }
                    btn(ui, "Geometrie-Prüfung...", 140.0, 2);
                });
            });
            hline(ui);
            pad(ui, 6.0, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if btn(ui, "Beenden", 80.0, 0) {
                            finish = true;
                        }
                        if btn(ui, "Schließen", 84.0, if pos < n { 0 } else { 2 }) {
                            close = true;
                        }
                    });
                });
            });
        });
        if finish {
            pos = n;
        }
        if pos != before && self.editor.is_none() && self.sketch.is_none() {
            self.data.insert_index = if pos >= n { None } else { Some(pos) };
            self.regenerate(false);
        }
        if info {
            if let Some(f) = self.data.features.get(pos.wrapping_sub(1)) {
                self.tree_selection = vec![f.id];
            }
            self.creo.tools = std::mem::take(t);
            self.feature_info();
            *t = std::mem::take(&mut self.creo.tools);
        }
        if dims {
            if let Some(id) = self.data.features.get(pos.wrapping_sub(1)).map(|f| f.id) {
                self.start_edit_dims(id);
            }
        }
        if close || finish || !open {
            self.creo.player = false;
        }
    }

    fn dlg_info(&mut self, ctx: &egui::Context, t: &mut ToolsUi) {
        let Some((title, text)) = t.info.clone() else { return };
        let (mut close, mut hit) = (false, None);
        let q = t.info_find.clone().unwrap_or_default().to_lowercase();
        let hits = if q.is_empty() { 0 } else { text.to_lowercase().matches(&q).count() };
        let open = cwin(ctx, "dlg_info", &title, true, pos2(420.0, 130.0), 864.0, |ui| {
            hit = menubar(
                ui,
                "info",
                &[
                    ("Datei", vec![M::I("Speichern", "", true), M::I("Speichern unter...", "", true), M::Sep, M::I("Schließen", "", true)]),
                    ("Editieren", vec![M::I("Kopieren", "Strg+C", true), M::I("Alle auswählen", "", true), M::I("Suchen...", "Strg+F", true)]),
                    ("Ansicht", vec![M::Chk("Zeilennummern", t.info_lines, true)]),
                ],
            );
            find_bar(ui, &mut t.info_find, hits);
            egui::Frame::NONE.fill(Color32::from_rgb(244, 245, 246)).inner_margin(2.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 0.0;
                egui::ScrollArea::both().id_salt("info_text").max_height(520.0).min_scrolled_height(520.0).show(ui, |ui| {
                    ui.set_min_size(vec2(ui.available_width(), 514.0));
                    let mono = egui::FontId::monospace(14.0);
                    for (i, l) in text.lines().enumerate() {
                        let shown = if t.info_lines { format!("{:>4}  {}", i + 1, l) } else { l.to_string() };
                        let g = ui.painter().layout_no_wrap(shown, mono.clone(), Color32::from_rgb(30, 32, 36));
                        let (r, _) = ui.allocate_exact_size(vec2(g.size().x.max(ui.available_width()), 18.0), Sense::hover());
                        if !q.is_empty() && l.to_lowercase().contains(&q) {
                            ui.painter().rect_filled(r, 0.0, Color32::from_rgb(255, 240, 150));
                        }
                        ui.painter().galley(r.min, g, Color32::from_rgb(30, 32, 36));
                    }
                });
            });
            pad(ui, 8.0, |ui| {
                ui.vertical_centered(|ui| {
                    if btn(ui, "Schließen", 84.0, 1) {
                        close = true;
                    }
                });
            });
        });
        match hit {
            Some("Schließen") => close = true,
            Some("Zeilennummern") => t.info_lines = !t.info_lines,
            Some("Kopieren") | Some("Alle auswählen") => {
                ctx.copy_text(text);
                self.say("Inhalt des Informationsfensters in die Zwischenablage kopiert.", MsgKind::Info);
            }
            Some("Suchen...") => t.info_find = Some(String::new()),
            Some("Speichern") | Some("Speichern unter...") => {
                let name = title.split('(').nth(1).map(|s| s.trim_end_matches(')').to_string()).unwrap_or_else(|| "info.txt".into());
                self.save_text("Informationen speichern", &name, &text);
            }
            Some(h) => self.todo(h),
            None => {}
        }
        if close || !open {
            t.info = None;
            t.info_find = None;
        }
    }
}

/// Replaces every occurrence of `find` in `s`, ignoring case.
fn replace_ci(s: &str, find: &str, repl: &str) -> String {
    let (lower, needle) = (s.to_lowercase(), find.to_lowercase());
    if needle.is_empty() || lower.len() != s.len() {
        return s.replace(find, repl);
    }
    let mut out = String::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find(&needle) {
        out.push_str(&s[from..from + i]);
        out.push_str(repl);
        from += i + needle.len();
    }
    out.push_str(&s[from..]);
    out
}
