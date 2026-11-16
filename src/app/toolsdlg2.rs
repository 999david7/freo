//! More dialogs of the Werkzeuge tab in Creo's layout: Referenz-Viewer, Such-Tool,
//! Komponentenschnittstelle, Publiziergeometrie, the UDF Menü-Manager, Hilfsanwendungen,
//! Bildeditor, Schraffur-Designer and the Farbeffekte-Manager.

use super::creo::fieldset;
use super::dlgkit::*;
use super::icons::{self, Icon};
use super::ribbon::theme::*;
use super::toolsdlg::ToolsUi;
use super::*;
use crate::model::assembly::AsmCType;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke, StrokeKind};

pub struct RefUi {
    pub tab: usize,
    pub deps: bool,
    pub combos: [usize; 5],
    pub types: [bool; 5],
    pub kids: [bool; 3],
    pub tree: bool,
    pub filt: usize,
    pub view: usize,
    pub tree_col: bool,
    pub collapsed: bool,
}
impl Default for RefUi {
    fn default() -> Self {
        RefUi { tab: 0, deps: false, combos: [0; 5], types: [true, true, true, true, false], kids: [true; 3], tree: false, filt: 1, view: 0, tree_col: false, collapsed: false }
    }
}

pub struct FindUi {
    pub what: usize,
    pub over: usize,
    pub tab: usize,
    pub rule: usize,
    pub cmp: usize,
    pub hits: Option<Vec<(u64, String)>>,
    pub opts: [bool; 5],
    pub saved: Option<String>,
}
impl Default for FindUi {
    fn default() -> Self {
        FindUi { what: 0, over: 0, tab: 0, rule: 0, cmp: 0, hits: None, opts: [true, false, true, false, false], saved: None }
    }
}

#[derive(Default)]
pub struct IfaceUi {
    pub draft: Option<Vec<IfaceItem>>,
    pub sel: Option<usize>,
    pub tab: usize,
    pub name: String,
    pub place: usize,
}

pub struct HatchUi {
    pub angle: String,
    pub x: String,
    pub y: String,
    pub dy: String,
    pub scale: f32,
    pub color: usize,
    pub style: usize,
}
impl Default for HatchUi {
    fn default() -> Self {
        HatchUi { angle: "45".into(), x: "0.000".into(), y: "0.000".into(), dy: "1.000".into(), scale: 1.0, color: 12, style: 0 }
    }
}

#[derive(Default)]
pub struct FarbUi {
    pub tab: usize,
    pub view: usize,
    pub sel: usize,
    pub tips: bool,
    pub search: String,
    pub key: String,
    pub desc: String,
    pub my: Vec<[u8; 3]>,
}

/// A registered Hilfsanwendung and its process while it runs.
pub struct AuxApp {
    pub name: String,
    pub path: PathBuf,
    pub child: Option<std::process::Child>,
}

/// Bildeditor: the picture being edited.
#[derive(Default)]
pub struct ImageUi {
    pub img: Option<image::RgbaImage>,
    pub tex: Option<egui::TextureHandle>,
    pub stale: bool,
    pub path: Option<PathBuf>,
    pub undo: Option<image::RgbaImage>,
    pub alpha: bool,
    pub cursor: bool,
    pub argb: bool,
}

const FIND_WHAT: [&str; 8] = ["KE", "Achse", "Bezugsebene", "Bezugspunkt", "Koordinatensystem", "Skizze", "Bohrung", "Rundung"];
const CMP: [&str; 6] = ["ist gleich", "ist nicht gleich", "ist kleiner als", "ist größer als", "ist kleiner oder gleich", "ist größer oder gleich"];
/// 40 Design-Farben of Creo's colour menus (4 rows of 10).
const DESIGN: [[u8; 3]; 40] = [
    [88, 88, 88],
    [240, 240, 250],
    [255, 255, 170],
    [255, 214, 170],
    [255, 170, 170],
    [255, 170, 230],
    [214, 204, 255],
    [170, 214, 255],
    [150, 250, 250],
    [170, 255, 180],
    [60, 60, 60],
    [196, 196, 200],
    [255, 255, 0],
    [255, 128, 20],
    [255, 0, 20],
    [255, 50, 160],
    [120, 50, 255],
    [0, 150, 240],
    [60, 220, 240],
    [0, 220, 60],
    [30, 30, 30],
    [140, 140, 144],
    [170, 160, 100],
    [200, 100, 60],
    [190, 80, 90],
    [200, 70, 140],
    [120, 120, 190],
    [80, 130, 180],
    [50, 150, 160],
    [110, 150, 100],
    [0, 0, 0],
    [110, 110, 114],
    [120, 100, 0],
    [130, 60, 10],
    [120, 0, 0],
    [120, 10, 100],
    [50, 20, 180],
    [0, 70, 150],
    [40, 100, 110],
    [0, 110, 20],
];
const MY_COLORS: [[u8; 3]; 15] = [
    [176, 188, 206],
    [226, 206, 150],
    [214, 172, 110],
    [220, 170, 90],
    [250, 170, 30],
    [240, 180, 30],
    [220, 150, 20],
    [240, 140, 20],
    [250, 130, 20],
    [180, 110, 60],
    [230, 180, 40],
    [250, 248, 220],
    [246, 220, 150],
    [252, 250, 210],
    [240, 250, 90],
];
const LIB_COLORS: [[u8; 3]; 15] = [
    [190, 140, 30],
    [130, 140, 160],
    [120, 130, 150],
    [190, 140, 30],
    [160, 60, 40],
    [150, 60, 40],
    [170, 170, 170],
    [190, 190, 190],
    [170, 80, 50],
    [160, 70, 50],
    [170, 150, 80],
    [160, 130, 60],
    [130, 130, 130],
    [110, 110, 110],
    [100, 96, 92],
];

/// `*` matches any run of characters; the comparison ignores case.
pub(super) fn glob(pat: &str, s: &str) -> bool {
    let (p, s): (Vec<char>, Vec<char>) = (pat.to_lowercase().chars().collect(), s.to_lowercase().chars().collect());
    let (mut i, mut j, mut star, mut mark) = (0, 0, None, 0);
    while j < s.len() {
        if i < p.len() && (p[i] == s[j] || p[i] == '?') {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == '*' {
            star = Some(i);
            mark = j;
            i += 1;
        } else if let Some(st) = star {
            i = st + 1;
            mark += 1;
            j = mark;
        } else {
            return false;
        }
    }
    p[i..].iter().all(|c| *c == '*')
}

fn row_combo(ui: &mut egui::Ui, label: &str, key: &str, cur: &mut usize, items: &[&str]) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(108.0, 32.0), Sense::hover());
        ui.painter().text(r.left_center() + vec2(2.0, 0.0), Align2::LEFT_CENTER, label, font(), TEXT);
        combo(ui, key, ui.available_width(), cur, items, true, None);
    });
}

/// Collector box of Publiziergeometrie.
fn collector(ui: &mut egui::Ui, title: &str, h: f32, active: bool, details: bool, count: usize) -> bool {
    lbl(ui, title);
    let mut hit = false;
    ui.horizontal(|ui| {
        let (r, resp) =
            white_box(ui, ui.available_width() - if details { 76.0 } else { 0.0 }, h, if active { Color32::from_rgb(232, 250, 214) } else { Color32::from_rgb(244, 242, 252) });
        ui.painter().with_clip_rect(r.shrink(2.0)).text(
            r.left_top() + vec2(22.0, 16.0),
            Align2::LEFT_CENTER,
            if count > 0 {
                format!("{} Element(e) ausgewählt", count)
            } else if active {
                "Elemente auswählen".to_string()
            } else {
                "Hier klicken, um Element hinzuzufügen".to_string()
            },
            font(),
            TEXT,
        );
        hit = resp.clicked();
        if details {
            ui.vertical(|ui| {
                ui.add_space((h - 32.0) / 2.0);
                btn(ui, "Details...", 66.0, 0);
            });
        }
    });
    hit
}

impl App {
    fn todo2(&mut self, name: &str) {
        self.say(format!("„{}“ ist in Freo noch nicht verfügbar.", name.trim_end_matches("...")), MsgKind::Info);
    }

    pub(super) fn dlg_ref_viewer(&mut self, ctx: &egui::Context, t: &mut ToolsUi) {
        if !self.creo.ref_viewer {
            return;
        }
        let rv = &mut t.refv;
        let model = format!("{}.PRT", self.data.name.to_uppercase());
        let cur = self.tree_selection.first().and_then(|id| self.data.feature(*id)).cloned();
        let (parents, children): (Vec<(Icon, String)>, Vec<(Icon, String)>) = match &cur {
            Some(f) => (
                f.kind.references().iter().filter_map(|id| self.data.feature(*id)).map(|p| (icons::for_feature(&p.kind), p.name.to_uppercase())).collect(),
                self.data.features.iter().filter(|c| c.kind.references().contains(&f.id)).map(|c| (icons::for_feature(&c.kind), c.name.to_uppercase())).collect(),
            ),
            None => (vec![], vec![]),
        };
        let links = self.data.features.iter().filter(|f| !f.kind.references().is_empty()).count();
        let (mut close, mut hit) = (false, None);
        let open = cwin(ctx, "dlg_refv", "Referenz-Viewer", true, pos2(200.0, 86.0), 1044.0, |ui| {
            hit = menubar(
                ui,
                "refv",
                &[
                    (
                        "Datei",
                        vec![
                            M::I("Pfade speichern", "Strg+S", true),
                            M::I("Pfade laden", "Strg+L", true),
                            M::Sub("Exportieren", true, vec![M::I("Als Text exportieren...", "", true), M::I("Als CSV exportieren...", "", true)]),
                            M::I("Schließen", "", true),
                        ],
                    ),
                    (
                        "Ansicht",
                        vec![
                            M::Radio("Eltern und Kinder", rv.view == 0),
                            M::Radio("Nur Eltern anzeigen", rv.view == 1),
                            M::Radio("Nur Kinder anzeigen", rv.view == 2),
                            M::Sep,
                            M::Chk("Baumspalte hinzufügen", rv.tree_col, true),
                            M::I("Alle erweitern", "", true),
                            M::I("Alle verkleinern", "", true),
                        ],
                    ),
                    (
                        "Aktionen",
                        vec![
                            M::I("Als aktuell festlegen", "", false),
                            M::Sub("Informationen", false, vec![]),
                            M::I("Pfade suchen...", "Strg+F", true),
                            M::I("Zirkuläre Pfade suchen", "", true),
                            M::Sub("In Modellbaum suchen", true, vec![M::I("Eltern im Modellbaum auswählen", "", true), M::I("Kinder im Modellbaum auswählen", "", true)]),
                        ],
                    ),
                ],
            );
            pad(ui, 6.0, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(330.0);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        tabs(ui, &mut rv.tab, &["Referenzen-Filter", "Pfade"], 9);
                        tab_page(ui, |ui| {
                            ui.set_min_height(540.0);
                            if rv.tab == 1 {
                                lbl(ui, "Benutzerdefinierte Pfade");
                                let (r, _) = white_box(ui, ui.available_width(), 214.0, Color32::WHITE);
                                let row = Rect::from_min_size(r.min + vec2(2.0, 2.0), vec2(r.width() - 50.0, 30.0));
                                ui.painter().rect_filled(row, 0.0, ROW_SEL);
                                ui.painter().text(row.left_center() + vec2(14.0, 0.0), Align2::LEFT_CENTER, "Neuer Pfad", font(), TEXT);
                                ui.vertical_centered(|ui| {
                                    if btn(ui, "Pfade suchen...", 100.0, 0) {
                                        hit = Some("Pfade suchen...");
                                    }
                                });
                                hline(ui);
                                lbl(ui, "Zirkuläre Pfade");
                                let (r, _) = white_box(ui, ui.available_width(), 204.0, Color32::WHITE);
                                ui.painter().text(r.left_top() + vec2(16.0, 18.0), Align2::LEFT_CENTER, "Zirkuläre Pfade aktualisieren", font(), TEXT);
                                ui.vertical_centered(|ui| {
                                    if btn(ui, "Zirkuläre Pfade suchen", 150.0, 0) {
                                        hit = Some("Zirkuläre Pfade suchen");
                                    }
                                });
                                return;
                            }
                            ui.horizontal_top(|ui| {
                                let (r, _) = ui.allocate_exact_size(vec2(96.0, 26.0), Sense::hover());
                                ui.painter().text(r.left_center() + vec2(2.0, 0.0), Align2::LEFT_CENTER, "Anzeigen:", font(), TEXT);
                                ui.vertical(|ui| {
                                    if radio(ui, !rv.deps, "Referenzen", true) {
                                        rv.deps = false;
                                    }
                                    if radio(ui, rv.deps, "Abhängigkeiten", true) {
                                        rv.deps = true;
                                    }
                                });
                            });
                            hline(ui);
                            if rv.deps {
                                row_combo(
                                    ui,
                                    "Status",
                                    "rv_dstat",
                                    &mut rv.combos[3],
                                    &[
                                        "Alle",
                                        "Fehlen",
                                        "Fehlende ohne Sicherung",
                                        "Nicht aktualisiert",
                                        "Für ausgeschlossene Eltern",
                                        "Für Auswechslungseltern",
                                        "Vollständig",
                                        "Unterbrechbar",
                                    ],
                                );
                                row_combo(
                                    ui,
                                    "Typ:",
                                    "rv_dtyp",
                                    &mut rv.combos[4],
                                    &[
                                        "Alle",
                                        "Familientabelle",
                                        "Auswechseln",
                                        "Austausch",
                                        "Mitgliedschaft",
                                        "Notizbuch deklariert",
                                        "Externe Darstellung",
                                        "Fertigung",
                                        "UDF",
                                        "Zusammenführen",
                                        "Konfigurierbare Baugruppe",
                                        "Dummy",
                                    ],
                                );
                            } else {
                                row_combo(ui, "Umfang", "rv_scope", &mut rv.combos[0], &["Alle", "Extern", "Lokal"]);
                                row_combo(
                                    ui,
                                    "Status",
                                    "rv_stat",
                                    &mut rv.combos[1],
                                    &["Alle", "Fehlen", "Fehlende ohne Sicherung", "Nicht aktualisiert", "Für ausgeschlossene Eltern", "Für Auswechslungseltern", "Vollständig"],
                                );
                                row_combo(ui, "Starke/schwache", "rv_strong", &mut rv.combos[2], &["Alle", "Stark", "Schwach"]);
                                lbl(ui, "Referenztypen:");
                                for (i, n) in ["Normal", "Platzierung", "Beziehung", "System", "Komponenten in Pfad"].into_iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.add_space(30.0);
                                        let (r, _) = ui.allocate_exact_size(vec2(18.0, 22.0), Sense::hover());
                                        let c = r.center();
                                        ui.painter().add(egui::Shape::line(
                                            vec![c + vec2(5.0, -5.0), c + vec2(-4.0, -5.0), c + vec2(4.0, 0.0), c + vec2(-5.0, 5.0), c + vec2(4.0, 5.0)],
                                            Stroke::new(2.4_f32, Color32::from_rgb(110, 114, 120)),
                                        ));
                                        ui.add_space(30.0);
                                        chk(ui, &mut rv.types[i], n, true);
                                    });
                                }
                            }
                            for (i, n) in ["Von unabhängigen Kindern", "Von ausgeschlossenen Kindern", "Von unterdrückten Kindern"].into_iter().enumerate() {
                                chk(ui, &mut rv.kids[i], n, true);
                            }
                            if rv.deps {
                                ui.add_space(238.0);
                            } else {
                                ui.add_space(42.0);
                            }
                            hline(ui);
                            chk(ui, &mut rv.tree, "Modellbaum filtern", true);
                            ui.horizontal(|ui| {
                                lbl(ui, "Filtern nach:");
                                combo(ui, "rv_filt", ui.available_width(), &mut rv.filt, &["Eltern- or Kindreferenzen", "Elternreferenzen", "Kind-Referenzen"], true, None);
                            });
                        });
                    });
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            lbl(ui, "Aktuelles Objekt:");
                            let (r, _) = ui.allocate_exact_size(vec2(210.0, 30.0), Sense::hover());
                            ui.painter().rect_filled(r, 0.0, Color32::from_rgb(244, 242, 252));
                            ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
                            let (ic, n) = match &cur {
                                Some(f) => (icons::for_feature(&f.kind), f.name.to_uppercase()),
                                None => (Icon::Part, model.clone()),
                            };
                            icons::paint(ui.painter(), Rect::from_center_size(r.left_center() + vec2(16.0, 0.0), vec2(16.0, 16.0)), ic, true);
                            ui.painter().with_clip_rect(r).text(r.left_center() + vec2(28.0, 0.0), Align2::LEFT_CENTER, &n, font(), TEXT);
                            let mut z = 0;
                            combo(ui, "rv_prev", 150.0, &mut z, &["Vorherig. verwenden"], false, None);
                        });
                        let w = ui.available_width();
                        let (strip, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
                        gradient(ui.painter(), strip, Color32::from_rgb(244, 245, 246), Color32::from_rgb(210, 214, 218));
                        ui.painter().text(strip.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, "Neuer Pfad:", font(), TEXT);
                        let chip = Rect::from_min_size(strip.left_top() + vec2(78.0, 3.0), vec2(124.0, 24.0));
                        ui.painter().rect_filled(chip, 0.0, Color32::from_rgb(232, 238, 252));
                        ui.painter().rect_stroke(chip, 0.0, Stroke::new(1.2_f32, Color32::from_rgb(60, 70, 200)), StrokeKind::Inside);
                        icons::paint(ui.painter(), Rect::from_center_size(chip.left_center() + vec2(12.0, 0.0), vec2(14.0, 14.0)), Icon::Part, true);
                        ui.painter().with_clip_rect(chip.shrink(2.0)).text(chip.left_center() + vec2(24.0, 0.0), Align2::LEFT_CENTER, &model, font(), TEXT);
                        // the graph: parents — current object — children
                        let (g, _) = ui.allocate_exact_size(vec2(w, 500.0), Sense::hover());
                        let p = ui.painter().with_clip_rect(g);
                        p.rect_filled(g, 0.0, Color32::WHITE);
                        p.rect_stroke(g, 0.0, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
                        let line = Stroke::new(1.0_f32, Color32::from_rgb(90, 96, 104));
                        let node = |c: egui::Pos2, ic: Icon, name: &str, wd: f32| -> Rect {
                            let r = Rect::from_center_size(c, vec2(wd, 24.0));
                            p.rect_filled(r, 2.0, Color32::from_rgb(232, 238, 250));
                            p.rect_stroke(r, 2.0, Stroke::new(1.0_f32, Color32::from_rgb(180, 192, 226)), StrokeKind::Inside);
                            icons::paint(&p, Rect::from_center_size(r.left_center() + vec2(12.0, 0.0), vec2(14.0, 14.0)), ic, true);
                            if wd > 40.0 {
                                p.with_clip_rect(Rect::from_min_max(r.min, pos2(r.right() - 20.0, r.bottom()))).text(
                                    r.left_center() + vec2(24.0, 0.0),
                                    Align2::LEFT_CENTER,
                                    name,
                                    font(),
                                    TEXT,
                                );
                            }
                            let d = r.right_center() - vec2(11.0, 0.0);
                            p.circle(d, 6.5, Color32::WHITE, Stroke::new(1.0_f32, Color32::from_rgb(60, 70, 200)));
                            for dy in [-2.5f32, 1.5] {
                                p.add(egui::Shape::line(
                                    vec![d + vec2(-3.0, dy - 1.5), d + vec2(0.0, dy + 1.5), d + vec2(3.0, dy - 1.5)],
                                    Stroke::new(1.2_f32, Color32::from_rgb(60, 70, 200)),
                                ));
                            }
                            r
                        };
                        let count = |c: egui::Pos2, n: usize| {
                            let r = Rect::from_center_size(c, vec2(16.0, 24.0));
                            p.rect_filled(r, 0.0, Color32::WHITE);
                            p.rect_stroke(r, 0.0, line, StrokeKind::Inside);
                            p.text(c, Align2::CENTER_CENTER, n.to_string(), font(), TEXT);
                        };
                        let bold = |at: egui::Pos2, s: &str| {
                            for dx in [0.0f32, 0.6] {
                                p.text(at + vec2(dx, 0.0), Align2::LEFT_CENTER, s, font(), TEXT);
                            }
                        };
                        let mid = pos2(g.center().x - 10.0, g.center().y - 10.0);
                        let title = cur.as_ref().map(|f| f.name.to_uppercase()).unwrap_or_else(|| model.clone());
                        bold(mid + vec2(-52.0, -40.0), &title);
                        let (show_p, show_c) = (rv.view != 2, rv.view != 1);
                        match &cur {
                            None => {
                                if rv.deps || links == 0 {
                                    icons::paint(&p, Rect::from_center_size(mid, vec2(18.0, 18.0)), Icon::Part, true);
                                    if show_p {
                                        bold(pos2(g.left() + 26.0, mid.y + 10.0), "Keine Eltern gefunden");
                                    }
                                    if show_c {
                                        bold(pos2(g.right() - 180.0, mid.y + 10.0), "Keine Kinder gefunden");
                                    }
                                } else {
                                    let c = node(mid, Icon::Part, "", 40.0);
                                    if show_p {
                                        let l = node(pos2(g.left() + 96.0, mid.y), Icon::Part, &model, 150.0);
                                        p.line_segment([l.right_center(), c.left_center()], line);
                                        count(pos2((l.right() + c.left()) / 2.0, mid.y), links);
                                    }
                                    if show_c {
                                        let r = node(pos2(g.right() - 100.0, mid.y), Icon::Part, &model, 164.0);
                                        p.line_segment([c.right_center(), r.left_center()], line);
                                        count(pos2((r.left() + c.right()) / 2.0, mid.y), links);
                                    }
                                }
                            }
                            Some(f) => {
                                let c = node(mid, icons::for_feature(&f.kind), "", 40.0);
                                let col = |list: &[(Icon, String)], x: f32, left: bool| {
                                    let n = list.len().min(12);
                                    for (i, (ic, name)) in list.iter().take(12).enumerate() {
                                        let y = mid.y + (i as f32 - (n as f32 - 1.0) / 2.0) * 32.0;
                                        let r = node(pos2(x, y), *ic, name, 160.0);
                                        if left {
                                            p.line_segment([r.right_center(), c.left_center()], line);
                                        } else {
                                            p.line_segment([c.right_center(), r.left_center()], line);
                                        }
                                    }
                                };
                                // Alle verkleinern folds each side into one node with the count
                                let fold = |list: &[(Icon, String)], what: &str| {
                                    if rv.collapsed && list.len() > 1 {
                                        vec![(Icon::FolderItem, format!("{} {}", list.len(), what))]
                                    } else {
                                        list.to_vec()
                                    }
                                };
                                if show_p {
                                    if parents.is_empty() {
                                        bold(pos2(g.left() + 26.0, mid.y + 10.0), "Keine Eltern gefunden");
                                    } else {
                                        col(&fold(&parents, "Eltern"), g.left() + 100.0, true);
                                    }
                                }
                                if show_c {
                                    if children.is_empty() {
                                        bold(pos2(g.right() - 180.0, mid.y + 10.0), "Keine Kinder gefunden");
                                    } else {
                                        col(&fold(&children, "Kinder"), g.right() - 104.0, false);
                                    }
                                }
                            }
                        }
                        let (foot, _) = ui.allocate_exact_size(vec2(w, 24.0), Sense::hover());
                        let third = foot.width() * 0.3;
                        for (x, s, x_mark) in
                            [(foot.left() + third * 0.5, "Eltern", true), (foot.center().x, "Aktuelles Objekt", false), (foot.right() - third * 0.55, "Kinder", true)]
                        {
                            ui.painter().text(pos2(x, foot.center().y), Align2::CENTER_CENTER, s, font(), TEXT);
                            if x_mark {
                                ui.painter().text(pos2(x + 70.0, foot.center().y), Align2::CENTER_CENTER, "×", egui::FontId::proportional(17.0), TEXT);
                            }
                        }
                        for x in [foot.left() + third, foot.right() - third] {
                            ui.painter().line_segment([pos2(x, foot.top() + 2.0), pos2(x, foot.bottom() - 2.0)], Stroke::new(1.0_f32, LINE));
                        }
                    });
                });
                ui.vertical_centered(|ui| {
                    if btn(ui, "Schließen", 80.0, 0) {
                        close = true;
                    }
                });
            });
        });
        match hit {
            Some("Schließen") => close = true,
            Some("Eltern und Kinder") => rv.view = 0,
            Some("Nur Eltern anzeigen") => rv.view = 1,
            Some("Nur Kinder anzeigen") => rv.view = 2,
            Some("Baumspalte hinzufügen") => rv.tree_col = !rv.tree_col,
            Some("Alle erweitern") => rv.collapsed = false,
            Some("Alle verkleinern") => rv.collapsed = true,
            Some(h) => {
                // every reference of the model as (child, parent)
                let pairs: Vec<(String, String)> = self
                    .data
                    .features
                    .iter()
                    .flat_map(|f| {
                        f.kind.references().into_iter().filter_map(|id| self.data.feature(id)).map(|p| (f.name.to_uppercase(), p.name.to_uppercase())).collect::<Vec<_>>()
                    })
                    .collect();
                let table = |sep: &str| pairs.iter().map(|(c, p)| format!("{}{}{}", p, sep, c)).collect::<Vec<_>>().join("\n");
                let name = self.data.name.to_lowercase();
                match h {
                    "Pfade speichern" | "Als Text exportieren..." => {
                        let s = format!("REFERENZPFADE FÜR {}\nElter -> Kind\n\n{}\n", model, table(" -> "));
                        self.save_text("Pfade speichern", &format!("{}_pfade.txt", name), &s);
                    }
                    "Als CSV exportieren..." => {
                        let s = format!("Elter;Kind\n{}\n", table(";"));
                        self.save_text("Referenzen exportieren", &format!("{}_referenzen.csv", name), &s);
                    }
                    "Pfade laden" => {
                        if let Some(s) = self.load_text("Pfade laden") {
                            t.info = Some(("Informationsfenster (pfade)".into(), s));
                        }
                    }
                    "Pfade suchen..." => {
                        let text = format!(
                            "REFERENZPFADE FÜR {}\n================================================\n\n{} Pfad(e) gefunden (Elter -> Kind):\n\n{}\n",
                            model,
                            pairs.len(),
                            table(" -> ")
                        );
                        t.info = Some(("Informationsfenster (pfade.inf)".into(), text));
                    }
                    "Zirkuläre Pfade suchen" => {
                        // a reference to a feature that comes later in the model closes a circle
                        let pos = |id: u64| self.data.features.iter().position(|f| f.id == id);
                        let n = self.data.features.iter().enumerate().filter(|(i, f)| f.kind.references().iter().any(|r| pos(*r).is_some_and(|p| p >= *i))).count();
                        self.say(
                            if n == 0 { "Es wurden keine zirkulären Pfade gefunden.".to_string() } else { format!("{} KE(s) mit zirkulären Referenzen gefunden.", n) },
                            if n == 0 { MsgKind::Info } else { MsgKind::Warning },
                        );
                    }
                    "Eltern im Modellbaum auswählen" | "Kinder im Modellbaum auswählen" => {
                        let ids: Vec<u64> = match &cur {
                            Some(f) if h.starts_with("Eltern") => f.kind.references(),
                            Some(f) => self.data.features.iter().filter(|c| c.kind.references().contains(&f.id)).map(|c| c.id).collect(),
                            None => vec![],
                        };
                        self.say(format!("{} KE(s) im Modellbaum ausgewählt.", ids.len()), MsgKind::Info);
                        if !ids.is_empty() {
                            self.tree_selection = ids;
                        }
                    }
                    h => self.todo2(h),
                }
            }
            None => {}
        }
        if close || !open {
            self.creo.ref_viewer = false;
        }
    }

    pub(super) fn dlg_search(&mut self, ctx: &egui::Context, t: &mut ToolsUi) {
        let Some(mut value) = self.creo.search.take() else { return };
        let fu = &mut t.find;
        let model = format!("{}.PRT", self.data.name.to_uppercase());
        let names: Vec<String> = self.data.features.iter().map(|f| f.name.to_uppercase()).collect();
        let (mut close, mut go, mut reset, mut opt) = (false, false, false, None);
        let open = cwin(ctx, "dlg_find", "Such-Tool:1", false, pos2(470.0, 110.0), 520.0, |ui| {
            pad(ui, 10.0, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(196.0);
                        lbl(ui, "Suchen nach:");
                        if combo(ui, "find_what", 196.0, &mut fu.what, &FIND_WHAT, true, None) {
                            fu.over = fu.what;
                        }
                        lbl(ui, "Suchen über:");
                        combo(ui, "find_over", 196.0, &mut fu.over, &FIND_WHAT, true, None);
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        lbl(ui, "Suchen in:");
                        let mut z = 0;
                        combo(ui, "find_in", ui.available_width(), &mut z, &[&model], false, None);
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ibox(ui, Icon::TlCursor, false, "");
                            ui.add_space(14.0);
                            let mut f = false;
                            chk(ui, &mut f, "Untermodelle einschließen", false);
                        });
                    });
                });
                ui.add_space(6.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                tabs(ui, &mut fu.tab, &["Attribute", "Verlauf", "Status", "Geometrie"], 3);
                tab_page(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(190.0);
                            fieldset(ui, "Regel", |ui| {
                                ui.set_min_height(196.0);
                                for (i, (n, en)) in
                                    [("Name", true), ("Typ", true), ("Ausdruck", true), ("Größe", false), ("Beschreibung", false), ("Eigenschaft", true)].into_iter().enumerate()
                                {
                                    if radio(ui, fu.rule == i, n, en) {
                                        fu.rule = i;
                                    }
                                }
                            });
                        });
                        ui.vertical(|ui| {
                            ui.set_width(ui.available_width());
                            fieldset(ui, "Kriterien", |ui| {
                                ui.set_min_height(196.0);
                                lbl(ui, "Vergleich:");
                                combo(ui, "find_cmp", ui.available_width(), &mut fu.cmp, &CMP, true, None);
                                lbl(ui, "Wert:");
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 0.0;
                                    field(ui, &mut value, ui.available_width() - 24.0, true);
                                    let (r, resp) = ui.allocate_exact_size(vec2(24.0, 32.0), Sense::click());
                                    ui.painter().rect_filled(r, 0.0, Color32::from_rgb(250, 250, 250));
                                    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
                                    super::ribbon::tri(ui.painter(), r.center() + vec2(0.0, 1.0), TEXT);
                                    let id = ui.id().with("find_values");
                                    if resp.clicked() {
                                        ui.memory_mut(|m| m.toggle_popup(id));
                                    }
                                    egui::popup_below_widget(ui, id, &resp, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                                        ui.set_min_width(220.0);
                                        egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                                            for n in &names {
                                                if ui.selectable_label(false, egui::RichText::new(n).size(F)).clicked() {
                                                    value = n.clone();
                                                }
                                            }
                                        });
                                    });
                                });
                            });
                        });
                    });
                });
                ui.add_space(6.0);
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let (r, arrow) = ui.allocate_exact_size(vec2(24.0, 32.0), Sense::click());
                        ui.painter().rect_filled(r, 0.0, Color32::from_rgb(250, 250, 250));
                        ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
                        super::ribbon::tri(ui.painter(), r.center() + vec2(0.0, 1.0), TEXT);
                        let b = ui.allocate_ui(vec2(84.0, 32.0), |ui| btn(ui, "Optionen", 84.0, 0)).response.interact(Sense::click());
                        let trigger = if b.clicked() { b } else { arrow };
                        opt = drop_rows(
                            ui,
                            "find_opts",
                            &trigger,
                            &[
                                M::Chk("Ausgeschlossen", true, false),
                                M::Chk("Unterdrückt", fu.opts[0], true),
                                M::Chk("Nicht abgerufen", false, false),
                                M::Chk("Nicht platziert", true, false),
                                M::Sep,
                                M::Chk("Baum filtern", fu.opts[1], true),
                                M::Chk("Elemente hervorheben", fu.opts[2], true),
                                M::Chk("Gefilterte Auswahlen", fu.opts[3], true),
                                M::Sep,
                                M::Chk("Abfrage erzeugen", fu.opts[4], true),
                                M::I("Abfrage speichern...", "", true),
                                M::I("Abfrage in Datei speichern...", "", true),
                                M::I("Abfragedatei abrufen...", "", true),
                                M::Sub(
                                    "Sortieren nach...",
                                    true,
                                    vec![
                                        M::I("Treffer nach Name sortieren", "", true),
                                        M::I("Treffer nach Typ sortieren", "", true),
                                        M::I("Treffer nach KE-Nummer sortieren", "", true),
                                    ],
                                ),
                            ],
                        );
                        ui.add_space(8.0);
                        if btn(ui, "Neue Suche", 96.0, 0) {
                            reset = true;
                        }
                        ui.add_space(6.0);
                        if btn(ui, "Jetzt suchen", 100.0, 1) {
                            go = true;
                        }
                    });
                });
                if let Some(hits) = &fu.hits {
                    fieldset(ui, &format!("{} Element(e) gefunden:", hits.len()), |ui| {
                        egui::Frame::NONE.fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, LINE)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = 0.0;
                            egui::ScrollArea::vertical().id_salt("find_hits").max_height(150.0).show(ui, |ui| {
                                ui.set_min_height(60.0);
                                for (id, n) in hits {
                                    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                                    if self.tree_selection.contains(id) {
                                        ui.painter().rect_filled(r, 0.0, ROW_SEL);
                                    } else if resp.hovered() {
                                        ui.painter().rect_filled(r, 0.0, HOVER_BG);
                                    }
                                    ui.painter().text(r.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, n, font(), TEXT);
                                    if resp.clicked() {
                                        self.tree_selection = vec![*id];
                                    }
                                }
                            });
                        });
                    });
                }
            });
            hline(ui);
            pad(ui, 10.0, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if btn(ui, "Schließen", 84.0, 0) {
                            close = true;
                        }
                    });
                });
            });
        });
        if let Some(o) = opt {
            match o {
                "Unterdrückt" => fu.opts[0] = !fu.opts[0],
                "Baum filtern" => fu.opts[1] = !fu.opts[1],
                "Elemente hervorheben" => fu.opts[2] = !fu.opts[2],
                "Gefilterte Auswahlen" => fu.opts[3] = !fu.opts[3],
                "Abfrage erzeugen" => fu.opts[4] = !fu.opts[4],
                "Abfrage speichern..." => {
                    fu.saved = Some(format!("nach={}\nregel={}\nvergleich={}\nwert={}\n", fu.what, fu.rule, fu.cmp, value));
                    self.say("Abfrage für diese Sitzung gespeichert.", MsgKind::Info);
                }
                "Abfrage in Datei speichern..." => {
                    let q = format!("nach={}\nregel={}\nvergleich={}\nwert={}\n", fu.what, fu.rule, fu.cmp, value);
                    self.save_text("Abfrage speichern", "abfrage.qry", &q);
                }
                "Abfragedatei abrufen..." => {
                    if let Some(s) = self.load_text("Abfragedatei abrufen").or_else(|| fu.saved.clone()) {
                        for l in s.lines() {
                            if let Some((k, v)) = l.split_once('=') {
                                match k {
                                    "nach" => {
                                        fu.what = v.parse().unwrap_or(0).min(FIND_WHAT.len() - 1);
                                        fu.over = fu.what;
                                    }
                                    "regel" => fu.rule = v.parse().unwrap_or(0).min(5),
                                    "vergleich" => fu.cmp = v.parse().unwrap_or(0).min(CMP.len() - 1),
                                    "wert" => value = v.to_string(),
                                    _ => {}
                                }
                            }
                        }
                        self.say("Abfrage geladen.", MsgKind::Info);
                    }
                }
                "Treffer nach Name sortieren" | "Treffer nach Typ sortieren" | "Treffer nach KE-Nummer sortieren" => {
                    let key =
                        |id: u64| self.data.features.iter().position(|f| f.id == id).map(|i| (i, self.data.features[i].kind.type_name().to_string())).unwrap_or((0, String::new()));
                    if let Some(h) = fu.hits.as_mut() {
                        match o {
                            "Treffer nach Name sortieren" => h.sort_by(|a, b| a.1.cmp(&b.1)),
                            "Treffer nach Typ sortieren" => h.sort_by_key(|x| key(x.0).1),
                            _ => h.sort_by_key(|x| key(x.0).0),
                        }
                    }
                }
                o => self.todo2(o),
            }
        }
        if reset {
            value = "*".into();
            t.find.hits = None;
            t.find.rule = 0;
            t.find.cmp = 0;
        }
        if go {
            let fu = &mut t.find;
            let kind_ok = |f: &Feature| match FIND_WHAT[fu.what] {
                "Achse" => matches!(f.kind, FeatureKind::DatumAxis(_)),
                "Bezugsebene" => matches!(f.kind, FeatureKind::DatumPlane(_)),
                "Bezugspunkt" => matches!(f.kind, FeatureKind::DatumPoint(_)),
                "Koordinatensystem" => matches!(f.kind, FeatureKind::Csys | FeatureKind::DatumCsys(_)),
                "Skizze" => matches!(f.kind, FeatureKind::Sketch(_)),
                "Bohrung" => matches!(f.kind, FeatureKind::Hole(_)),
                "Rundung" => matches!(f.kind, FeatureKind::Round(_)),
                _ => true,
            };
            let hits: Vec<(u64, String)> = self
                .data
                .features
                .iter()
                .enumerate()
                .filter(|(_, f)| kind_ok(f) && (fu.opts[0] || !f.suppressed))
                .filter(|(i, f)| {
                    let m = match fu.rule {
                        1 => glob(&value, f.kind.type_name()),
                        2 | 5 => glob(&value, &f.id.to_string()) || glob(&value, &(i + 1).to_string()),
                        _ => glob(&value, &f.name),
                    };
                    if fu.cmp == 1 {
                        !m
                    } else {
                        m
                    }
                })
                .map(|(_, f)| (f.id, f.name.to_uppercase()))
                .collect();
            if fu.opts[2] {
                self.tree_selection = hits.iter().map(|h| h.0).collect();
            }
            self.say(format!("{} Element(e) gefunden.", hits.len()), MsgKind::Info);
            fu.hits = Some(hits);
        }
        if close || !open {
            t.find.hits = None;
        } else {
            self.creo.search = Some(value);
        }
    }

    /// Komponentenschnittstelle: placement references this part brings into assemblies.
    pub(super) fn dlg_interface(&mut self, ctx: &egui::Context, t: &mut ToolsUi) {
        if !self.creo.iface_dialog {
            t.iface.draft = None;
            return;
        }
        let iu = &mut t.iface;
        if iu.draft.is_none() {
            iu.draft = Some(self.data.interface.clone());
            iu.sel = None;
            if iu.name.is_empty() {
                iu.name = "INTFC001".into();
            }
        }
        let mut items = iu.draft.clone().unwrap_or_default();
        let (mut ok, mut cancel, mut add) = (false, false, false);
        let open = cwin(ctx, "dlg_iface", "KOMPONENTENSCHNITTSTELLE", false, pos2(440.0, 160.0), 583.0, |ui| {
            pad(ui, 12.0, |ui| {
                ui.add_space(10.0);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    egui::Frame::NONE.fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, LINE)).show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(262.0);
                            ui.set_min_height(244.0);
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let (r, resp) = ui.allocate_exact_size(vec2(262.0, 24.0), Sense::click());
                            if iu.sel.is_none() {
                                ui.painter().rect_filled(r, 0.0, ROW_SEL);
                            }
                            ui.painter().text(r.left_center() + vec2(16.0, 0.0), Align2::LEFT_CENTER, &iu.name, font(), TEXT);
                            if resp.clicked() {
                                iu.sel = None;
                            }
                            let mut del = None;
                            for (i, it) in items.iter().enumerate() {
                                let (r, resp) = ui.allocate_exact_size(vec2(262.0, 24.0), Sense::click());
                                if iu.sel == Some(i) {
                                    ui.painter().rect_filled(r, 0.0, ROW_SEL);
                                } else if resp.hovered() {
                                    ui.painter().rect_filled(r, 0.0, HOVER_BG);
                                }
                                icons::paint(ui.painter(), Rect::from_center_size(r.left_center() + vec2(20.0, 0.0), vec2(12.0, 12.0)), Icon::TlPromptArrow, true);
                                ui.painter().with_clip_rect(r).text(
                                    r.left_center() + vec2(36.0, 0.0),
                                    Align2::LEFT_CENTER,
                                    format!("{} – {}", it.ty.name(), it.name),
                                    font(),
                                    TEXT,
                                );
                                if resp.clicked() {
                                    iu.sel = Some(i);
                                }
                                resp.context_menu(|ui| {
                                    if ui.button("Löschen").clicked() {
                                        del = Some(i);
                                        ui.close_menu();
                                    }
                                });
                            }
                            if let Some(i) = del {
                                items.remove(i);
                                iu.sel = None;
                            }
                            if items.is_empty() {
                                let (r, _) = ui.allocate_exact_size(vec2(262.0, 24.0), Sense::hover());
                                icons::paint(ui.painter(), Rect::from_center_size(r.left_center() + vec2(20.0, 0.0), vec2(12.0, 12.0)), Icon::TlPromptArrow, true);
                                ui.painter().text(r.left_center() + vec2(36.0, 0.0), Align2::LEFT_CENTER, "Automatisch", font(), TEXT);
                            }
                            let (r, resp) = ui.allocate_exact_size(vec2(262.0, 24.0), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(r, 0.0, HOVER_BG);
                            }
                            ui.painter().text(r.left_center() + vec2(26.0, 0.0), Align2::LEFT_CENTER, "Neue Bedingung", font(), TEXT);
                            if resp.on_hover_text("Ebenen, ebene oder zylindrische Flächen, Achsen oder Punkte im Grafikfenster wählen, dann hier klicken").clicked() {
                                add = true;
                            }
                        });
                    });
                    egui::Frame::NONE.stroke(Stroke::new(1.0_f32, LINE)).show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(ui.available_width());
                            ui.set_min_height(244.0);
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
                            ui.painter().text(r.center(), Align2::CENTER_CENTER, "Eigenschaften", font(), Color32::from_rgb(110, 114, 120));
                            ui.painter().line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0_f32, LINE));
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                let w = ui.available_width() / 2.0;
                                for (k, n) in ["Platzierung", "Kriterien"].into_iter().enumerate() {
                                    let (r, resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::click());
                                    if iu.tab != k {
                                        ui.painter().rect_filled(r, 0.0, Color32::from_rgb(232, 234, 236));
                                        ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
                                    }
                                    ui.painter().text(r.center(), Align2::CENTER_CENTER, n, font(), if iu.tab == k { TEXT } else { Color32::from_rgb(110, 114, 120) });
                                    if resp.clicked() {
                                        iu.tab = k;
                                    }
                                }
                            });
                            pad(ui, 6.0, |ui| {
                                if iu.tab == 1 {
                                    lbl_dim(ui, "Keine Kriterien definiert.");
                                    return;
                                }
                                match iu.sel.filter(|i| *i < items.len()) {
                                    None => {
                                        let mut on = true;
                                        chk(ui, &mut on, "Als Standardschnittstelle einstellen", false);
                                        lbl(ui, "Name der Schnittstelle");
                                        field(ui, &mut iu.name, 150.0, true);
                                        lbl(ui, "Schnittstellenschablone");
                                        let mut z = 0;
                                        combo(ui, "if_tmpl", 144.0, &mut z, &["Benutzerdefiniert"], true, None);
                                        lbl(ui, "Platzierung/Ziel-Schnittstelle");
                                        combo(ui, "if_place", 144.0, &mut iu.place, &["Beide(s)", "Platzieren", "Empfangen"], true, None);
                                    }
                                    Some(i) => {
                                        lbl(ui, "Bedingungstyp");
                                        let types = [AsmCType::Coincident, AsmCType::Distance, AsmCType::Parallel, AsmCType::Normal, AsmCType::Angle];
                                        let names: Vec<&str> = types.iter().map(|t| t.name()).collect();
                                        let mut k = types.iter().position(|t| *t == items[i].ty).unwrap_or(0);
                                        if combo(ui, "if_type", 180.0, &mut k, &names, true, None) {
                                            items[i].ty = types[k];
                                        }
                                        lbl(ui, "Komponentenreferenz");
                                        readout(ui, &items[i].name, 180.0);
                                        if btn(ui, "Bedingung löschen", 130.0, 0) {
                                            items.remove(i);
                                            iu.sel = None;
                                        }
                                    }
                                }
                            });
                        });
                    });
                });
            });
            hline(ui);
            pad(ui, 10.0, |ui| {
                ui.horizontal(|ui| {
                    (ok, cancel) = ok_cancel_icons(ui);
                });
            });
        });
        if add {
            if self.selection.is_empty() {
                self.say("Referenzen im Modell wählen, dann „Neue Bedingung“ klicken.", MsgKind::Prompt);
            }
            let topo = self.result.topo.clone();
            for p in self.selection.clone() {
                let it = match p {
                    Pick::Plane(id) => Some((IfaceRef::Plane(id), self.data.feature(id).map(|f| f.name.clone()).unwrap_or_default())),
                    Pick::Axis(id) => Some((IfaceRef::Axis(id), self.result.axis(id).map(|a| a.name.clone()).unwrap_or_default())),
                    Pick::Point(id) => Some((IfaceRef::Point(id), self.data.feature(id).map(|f| f.name.clone()).unwrap_or_default())),
                    Pick::Face(f) => {
                        let planar = topo.faces.get(f).map(|x| x.planar).unwrap_or(true);
                        self.face_ref(p).map(|fr| (IfaceRef::Surface { face: fr, planar }, if planar { "Ebene Fläche".to_string() } else { "Zylinderfläche".to_string() }))
                    }
                    _ => None,
                };
                if let Some((r, name)) = it {
                    if !items.iter().any(|x| x.r == r) {
                        items.push(IfaceItem { ty: AsmCType::Coincident, r, name });
                    }
                }
            }
        }
        if ok && items != self.data.interface {
            self.commit("Komponentenschnittstelle geändert.", |d| d.interface = items.clone());
        }
        if ok || cancel || !open {
            self.creo.iface_dialog = false;
            t.iface.draft = None;
        } else {
            t.iface.draft = Some(items);
        }
    }

    pub(super) fn dlg_publish(&mut self, ctx: &egui::Context, t: &mut ToolsUi) {
        if !t.publish {
            return;
        }
        if t.publish_name.is_empty() {
            t.publish_name = format!("PUBLIZIERTE_GEOMETRIE_{}", self.data.published.len() + 1);
        }
        let (mut ok, mut cancel) = (false, false);
        // what is selected in the graphics window goes into the collectors
        let faces: Vec<_> = self.selection.iter().filter(|p| matches!(p, Pick::Face(_))).filter_map(|p| self.face_ref(*p)).collect();
        let edges = self.selection.iter().filter(|p| matches!(p, Pick::Edge(_))).count();
        let refs: Vec<u64> = self
            .selection
            .iter()
            .filter_map(|p| match p {
                Pick::Plane(id) | Pick::Axis(id) | Pick::Point(id) => Some(*id),
                _ => None,
            })
            .collect();
        let counts = [faces.len(), 0, edges, refs.len()];
        let open = cwin(ctx, "dlg_publish", "Publiziergeometrie", false, pos2(360.0, 96.0), 386.0, |ui| {
            pad(ui, 12.0, |ui| {
                ui.add_space(8.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                tabs(ui, &mut t.publish_tab, &["Referenzen", "Eigenschaften"], 9);
                tab_page(ui, |ui| {
                    ui.set_min_height(640.0);
                    if t.publish_tab == 1 {
                        ui.horizontal(|ui| {
                            lbl(ui, "Name");
                            field(ui, &mut t.publish_name, 150.0, true);
                            let (r, _) = ui.allocate_exact_size(vec2(22.0, 26.0), Sense::hover());
                            ui.painter().rect_filled(Rect::from_center_size(r.center(), vec2(14.0, 20.0)), 2.0, Color32::from_rgb(20, 96, 132));
                            ui.painter().text(r.center(), Align2::CENTER_CENTER, "i", font(), Color32::WHITE);
                        });
                        return;
                    }
                    lbl(ui, "Referenzmodell");
                    let mut s = String::new();
                    ui.add_enabled_ui(false, |ui| ui.add_sized(vec2(230.0, 30.0), egui::TextEdit::singleline(&mut s).hint_text("Keine Elemente").margin(vec2(8.0, 6.0))));
                    hline(ui);
                    for (i, (n, h)) in [("Flächensätze", 112.0f32), ("Körper", 64.0), ("Kette", 112.0), ("Referenzen", 116.0)].into_iter().enumerate() {
                        if collector(ui, n, h, t.publish_sel == i, true, counts[i]) {
                            t.publish_sel = i;
                        }
                    }
                    lbl(ui, "Anmerkungen");
                    ui.horizontal(|ui| {
                        lbl(ui, "0 Elemente ausgewählt");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            btn(ui, "Editieren", 62.0, 0);
                        });
                    });
                });
            });
            hline(ui);
            pad(ui, 10.0, |ui| {
                ui.horizontal(|ui| {
                    (ok, cancel) = ok_cancel_icons(ui);
                });
            });
        });
        if ok {
            if faces.is_empty() && edges == 0 && refs.is_empty() {
                self.say("Publiziergeometrie: Zuerst Flächen, Kanten oder Bezüge im Grafikfenster auswählen.", MsgKind::Warning);
                ok = false;
            } else {
                let g = PublishedGeom { name: t.publish_name.clone(), faces, edges, refs };
                let label = format!("{} wurde erzeugt.", g.name);
                self.commit(&label, |d| d.published.push(g));
                t.publish_name.clear();
            }
        }
        if ok || cancel || !open {
            t.publish = false;
        }
    }

    /// Creo's Menü-Manager with the UDF menu.
    pub(super) fn dlg_udf(&mut self, ctx: &egui::Context, t: &mut ToolsUi) {
        let Some(db) = t.udf else { return };
        let mut hit: Option<(bool, &str)> = None;
        let frame = egui::Frame::window(&ctx.style()).fill(Color32::WHITE).inner_margin(0.0).corner_radius(3.0).stroke(Stroke::new(1.0_f32, Color32::from_rgb(150, 156, 164)));
        egui::Window::new("Menü-Manager").id(egui::Id::new("dlg_udf")).title_bar(false).collapsible(false).resizable(false).default_pos(pos2(1040.0, 150.0)).frame(frame).show(
            ctx,
            |ui| {
                ui.set_width(154.0);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                let (r, _) = ui.allocate_exact_size(vec2(154.0, 32.0), Sense::hover());
                ui.painter().rect_filled(r, egui::CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, Color32::from_rgb(218, 222, 224));
                ui.painter().text(r.center(), Align2::CENTER_CENTER, "Menü-Manager", font(), TEXT);
                let band = |ui: &mut egui::Ui, name: &str| {
                    let (r, _) = ui.allocate_exact_size(vec2(154.0, 30.0), Sense::hover());
                    ui.painter().rect_filled(r, 0.0, Color32::from_rgb(196, 200, 204));
                    let c = r.left_center() + vec2(15.0, 0.0);
                    ui.painter().add(egui::Shape::convex_polygon(vec![c + vec2(-6.0, -3.0), c + vec2(6.0, -3.0), c + vec2(0.0, 4.0)], Color32::from_rgb(30, 32, 36), Stroke::NONE));
                    ui.painter().text(r.left_center() + vec2(36.0, 0.0), Align2::LEFT_CENTER, name, font(), TEXT);
                };
                let item = |ui: &mut egui::Ui, name: &str, style: u8| -> bool {
                    // style: 0 normal, 1 chosen (dark), 2 not available, 3 bold
                    let (r, resp) = ui.allocate_exact_size(vec2(154.0, 30.0), if style == 2 { Sense::hover() } else { Sense::click() });
                    if style == 1 {
                        ui.painter().rect_filled(r, 0.0, Color32::from_rgb(84, 88, 92));
                    } else if style != 2 && resp.hovered() {
                        ui.painter().rect_filled(r, 0.0, Color32::from_rgb(214, 238, 252));
                    }
                    let col = match style {
                        1 => Color32::WHITE,
                        2 => DIM_TEXT,
                        _ => TEXT,
                    };
                    ui.painter().text(r.left_center() + vec2(28.0, 0.0), Align2::LEFT_CENTER, name, font(), col);
                    if style == 3 {
                        ui.painter().text(r.left_center() + vec2(28.6, 0.0), Align2::LEFT_CENTER, name, font(), col);
                    }
                    resp.clicked()
                };
                band(ui, "UDF");
                for n in ["Erzeugen", "Ändern", "Auflisten", "Datenbank", "Integrieren", "Fertig/Zurück"] {
                    let style = if n == "Datenbank" && db {
                        1
                    } else if n == "Fertig/Zurück" && !db {
                        3
                    } else {
                        0
                    };
                    if item(ui, n, style) {
                        hit = Some((false, n));
                    }
                }
                if db {
                    band(ui, "DATENBANK");
                    for n in ["Speichern", "Speichern als", "Sicherung", "Umbenennen", "Löschen", "Unterdr löschen", "Aufräumen", "Varianten DB", "Alle löschen", "Fertig/Zurück"]
                    {
                        if item(
                            ui,
                            n,
                            match n {
                                "Varianten DB" => 2,
                                "Fertig/Zurück" => 3,
                                _ => 0,
                            },
                        ) {
                            hit = Some((true, n));
                        }
                    }
                }
            },
        );
        match hit {
            Some((false, "Fertig/Zurück")) => t.udf = None,
            Some((true, "Fertig/Zurück")) => t.udf = Some(false),
            Some((false, "Datenbank")) => t.udf = Some(!db),
            Some((false, "Erzeugen")) => {
                t.udf = None;
                self.open_file_task(super::modelcmds::FileTask::Udf);
            }
            Some((false, "Auflisten")) => {
                let dir = self.work_dir();
                let mut names: Vec<String> = std::fs::read_dir(&dir)
                    .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.to_lowercase().ends_with(".gph")).collect())
                    .unwrap_or_default();
                names.sort();
                let text = format!(
                    "UDF-BIBLIOTHEK\nVerzeichnis: {}\n================================================\n\n{}\n",
                    dir.display(),
                    if names.is_empty() { "Die Bibliothek enthält keine UDFs.".to_string() } else { names.join("\n") }
                );
                t.info = Some(("Informationsfenster (udf.lst)".into(), text));
            }
            Some((true, "Aufräumen")) => self.say("Aufräumen: Es sind keine alten Versionen von UDF-Dateien vorhanden.", MsgKind::Info),
            Some((true, "Unterdr löschen")) => self.say("Die UDFs der Bibliothek enthalten keine unterdrückten KEs.", MsgKind::Info),
            Some((true, n @ ("Löschen" | "Umbenennen"))) => {
                if let Some(p) = rfd::FileDialog::new().set_title(format!("UDF {}", n.to_lowercase())).set_directory(self.work_dir()).add_filter("UDF", &["gph"]).pick_file() {
                    let file = p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                    if n == "Löschen" {
                        if rfd::MessageDialog::new()
                            .set_title("UDF löschen")
                            .set_description(format!("{} wirklich löschen? Das lässt sich nicht rückgängig machen.", file))
                            .set_buttons(rfd::MessageButtons::YesNo)
                            .show()
                            == rfd::MessageDialogResult::Yes
                        {
                            match std::fs::remove_file(&p) {
                                Ok(_) => self.say(format!("UDF {} gelöscht.", file), MsgKind::Info),
                                Err(e) => self.say(format!("Löschen fehlgeschlagen: {}", e), MsgKind::Error),
                            }
                        }
                    } else if let Some(to) = rfd::FileDialog::new().set_title("Neuer Name des UDF").set_directory(self.work_dir()).set_file_name(&file).save_file() {
                        match std::fs::rename(&p, &to) {
                            Ok(_) => self.say(format!("UDF {} umbenannt.", file), MsgKind::Info),
                            Err(e) => self.say(format!("Umbenennen fehlgeschlagen: {}", e), MsgKind::Error),
                        }
                    }
                }
            }
            Some((true, "Alle löschen")) => {
                let dir = self.work_dir();
                let files: Vec<PathBuf> = std::fs::read_dir(&dir)
                    .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("gph"))).collect())
                    .unwrap_or_default();
                if files.is_empty() {
                    self.say("Die Bibliothek enthält keine UDFs.", MsgKind::Info);
                } else if rfd::MessageDialog::new()
                    .set_title("Alle UDFs löschen")
                    .set_description(format!("{} UDF-Datei(en) in {} wirklich löschen? Das lässt sich nicht rückgängig machen.", files.len(), dir.display()))
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show()
                    == rfd::MessageDialogResult::Yes
                {
                    let n = files.iter().filter(|p| std::fs::remove_file(p).is_ok()).count();
                    self.say(format!("{} UDF-Datei(en) gelöscht.", n), MsgKind::Info);
                }
            }
            Some((true, "Sicherung")) => {
                if let Some(to) = rfd::FileDialog::new().set_title("Sicherungsverzeichnis für UDFs").pick_folder() {
                    let n = std::fs::read_dir(self.work_dir())
                        .map(|rd| {
                            rd.filter_map(|e| e.ok())
                                .map(|e| e.path())
                                .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("gph")))
                                .filter(|p| p.file_name().is_some_and(|f| std::fs::copy(p, to.join(f)).is_ok()))
                                .count()
                        })
                        .unwrap_or(0);
                    self.say(format!("{} UDF-Datei(en) nach {} gesichert.", n, to.display()), MsgKind::Info);
                }
            }
            // Ändern, Integrieren, Speichern, Speichern als work in the UDF library dialog
            Some((_, _)) => {
                t.udf = None;
                self.open_file_task(super::modelcmds::FileTask::Udf);
            }
            None => {}
        }
    }

    pub(super) fn dlg_aux(&mut self, ctx: &egui::Context, t: &mut ToolsUi) {
        if !t.aux {
            return;
        }
        // a program that ended by itself is no longer running
        for a in t.aux_apps.iter_mut() {
            if a.child.as_mut().is_some_and(|c| !matches!(c.try_wait(), Ok(None))) {
                a.child = None;
            }
        }
        let (mut close, mut info, mut reg, mut start, mut stop, mut del) = (false, false, false, false, false, false);
        let sel = t.aux_sel.filter(|i| *i < t.aux_apps.len());
        let running = sel.is_some_and(|i| t.aux_apps[i].child.is_some());
        let open = cwin(ctx, "dlg_aux", "Hilfsanwendungen", false, pos2(300.0, 200.0), 408.0, |ui| {
            pad(ui, 12.0, |ui| {
                ui.add_space(6.0);
                ui.horizontal_top(|ui| {
                    egui::Frame::NONE.fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, LINE)).show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(250.0);
                            ui.set_min_height(184.0);
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let (r, _) = ui.allocate_exact_size(vec2(250.0, 30.0), Sense::hover());
                            ui.painter().text(r.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, "Name", font(), TEXT);
                            ui.painter().text(r.left_center() + vec2(148.0, 0.0), Align2::LEFT_CENTER, "Status", font(), TEXT);
                            ui.painter().line_segment([r.left_top() + vec2(140.0, 2.0), r.left_bottom() + vec2(140.0, -2.0)], Stroke::new(1.0_f32, LINE));
                            ui.painter().line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0_f32, Color32::from_rgb(226, 228, 230)));
                            for (i, a) in t.aux_apps.iter().enumerate() {
                                let (r, resp) = ui.allocate_exact_size(vec2(250.0, 26.0), Sense::click());
                                if sel == Some(i) {
                                    ui.painter().rect_filled(r, 0.0, ROW_SEL);
                                }
                                ui.painter().with_clip_rect(Rect::from_min_size(r.min, vec2(138.0, 26.0))).text(
                                    r.left_center() + vec2(10.0, 0.0),
                                    Align2::LEFT_CENTER,
                                    &a.name,
                                    font(),
                                    TEXT,
                                );
                                ui.painter().with_clip_rect(r).text(
                                    r.left_center() + vec2(148.0, 0.0),
                                    Align2::LEFT_CENTER,
                                    if a.child.is_some() { "Wird ausgeführt" } else { "Nicht gestartet" },
                                    font(),
                                    TEXT,
                                );
                                if resp.clicked() {
                                    t.aux_sel = Some(i);
                                }
                            }
                        });
                    });
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 5.0;
                        let w = ui.available_width();
                        if btn(ui, "Start", w, if sel.is_some() && !running { 0 } else { 2 }) {
                            start = true;
                        }
                        if btn(ui, "Anhalten", w, if running { 0 } else { 2 }) {
                            stop = true;
                        }
                        if btn(ui, "Löschen", w, if sel.is_some() && !running { 0 } else { 2 }) {
                            del = true;
                        }
                        if btn(ui, "Registrieren...", w, 0) {
                            reg = true;
                        }
                        if btn(ui, "Informationen", w, 0) {
                            info = true;
                        }
                    });
                });
            });
            hline(ui);
            pad(ui, 10.0, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if btn(ui, "Schließen", 84.0, 0) {
                            close = true;
                        }
                    });
                });
            });
        });
        if reg {
            if let Some(p) = rfd::FileDialog::new().set_title("Hilfsanwendung registrieren").pick_file() {
                let name = p.file_stem().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_default();
                t.aux_apps.push(AuxApp { name, path: p, child: None });
                t.aux_sel = Some(t.aux_apps.len() - 1);
            }
        }
        if let Some(i) = sel {
            if start {
                let a = &mut t.aux_apps[i];
                // an .app bundle is opened through the system, everything else is run directly
                let res = if a.path.extension().is_some_and(|x| x == "app") {
                    std::process::Command::new("open").arg("-W").arg(&a.path).spawn()
                } else {
                    std::process::Command::new(&a.path).spawn()
                };
                match res {
                    Ok(c) => {
                        a.child = Some(c);
                        let n = a.name.clone();
                        self.say(format!("Hilfsanwendung {} gestartet.", n), MsgKind::Info);
                    }
                    Err(e) => self.say(format!("Hilfsanwendung konnte nicht gestartet werden: {}", e), MsgKind::Error),
                }
            }
            if stop {
                if let Some(mut c) = t.aux_apps[i].child.take() {
                    let _ = c.kill();
                    let _ = c.wait();
                    self.say(format!("Hilfsanwendung {} angehalten.", t.aux_apps[i].name), MsgKind::Info);
                }
            }
            if del {
                t.aux_apps.remove(i);
                t.aux_sel = None;
            }
        }
        if info {
            let mut text = "==================================================\nHilfsanwendungs-Informationen\n==================================================\n\n".to_string();
            if t.aux_apps.is_empty() {
                text += "Es sind keine Hilfsanwendungen registriert.\n";
            }
            for (i, a) in t.aux_apps.iter().enumerate() {
                text += &format!(
                    "Anwendung {}: {}\nAusführungspfad: {}\n{}\n\n",
                    i + 1,
                    a.name,
                    a.path.display(),
                    if a.child.is_some() { "Anwendung wird ausgeführt." } else { "Anwendung wird nicht ausgeführt." }
                );
            }
            t.info = Some(("Informationsfenster (hilfsanwendungen.inf)".into(), text));
        }
        if close || !open {
            t.aux = false;
        }
    }

    pub(super) fn dlg_image(&mut self, ctx: &egui::Context, t: &mut ToolsUi) {
        if !t.image {
            return;
        }
        use image::imageops;
        let iu = &mut t.image_ui;
        let d = iu.img.is_some();
        let mut hit = None;
        let mut status = String::new();
        if iu.stale || (iu.tex.is_none() && iu.img.is_some()) {
            iu.stale = false;
            iu.tex = iu.img.as_ref().map(|im| {
                ctx.load_texture("bildeditor", egui::ColorImage::from_rgba_unmultiplied([im.width() as usize, im.height() as usize], im.as_raw()), egui::TextureOptions::LINEAR)
            });
        }
        let open = cwin(ctx, "dlg_image", "Bildeditor", true, pos2(380.0, 130.0), 665.0, |ui| {
            hit = menubar(
                ui,
                "image",
                &[
                    (
                        "Datei",
                        vec![
                            M::I("Neu", "", true),
                            M::I("Öffnen...", "", true),
                            M::I("Neu laden", "", d && iu.path.is_some()),
                            M::I("Speichern", "", d && iu.path.is_some()),
                            M::I("Speichern als...", "", d),
                            M::Sep,
                            M::I("Schließen", "", true),
                        ],
                    ),
                    ("Editieren", vec![M::I("Widerrufen", "", iu.undo.is_some()), M::Sep, M::I("Ausschneiden", "", d), M::I("Kopieren", "", d), M::I("Einfügen", "", false)]),
                    ("Ansicht", vec![M::Chk("Alphakanal anzeigen", iu.alpha, d), M::Chk("Cursor-Position", iu.cursor, d), M::Chk("ARGB-Wert des Pixels", iu.argb, d)]),
                    (
                        "Bild",
                        vec![
                            M::Sub("Größe...", d, vec![M::I("Auf 50 % verkleinern", "", true), M::I("Auf 200 % vergrößern", "", true)]),
                            M::Sub("Spiegeln", d, vec![M::I("Horizontal spiegeln", "", true), M::I("Vertikal spiegeln", "", true)]),
                            M::Sub("Rotieren", d, vec![M::I("90° im Uhrzeigersinn", "", true), M::I("90° gegen den Uhrzeigersinn", "", true), M::I("180°", "", true)]),
                            M::I("Zuschneiden", "", d),
                            M::I("Wiederholen...", "", d),
                            M::Sep,
                            M::I("Unscharf machen...", "", d),
                            M::I("Mit Ölfarbe malen...", "", true),
                            M::I("Verwischen...", "", d),
                            M::I("Schärfen...", "", d),
                            M::I("Aufhellen...", "", d),
                            M::I("Kontrast...", "", d),
                            M::I("Sättigung...", "", d),
                            M::I("Tönung...", "", d),
                            M::I("Grauskala", "", d),
                            M::I("Prägen", "", d),
                            M::I("Alphakanal erzeugen...", "", d),
                            M::Sep,
                            M::I("Dia-Show...", "", true),
                        ],
                    ),
                ],
            );
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                ui.spacing_mut().item_spacing.x = 4.0;
                for (ic, cmd, tip) in [
                    (Icon::TlHalfSize, "Auf 50 % verkleinern", "Bild verkleinern"),
                    (Icon::TlDoubleSize, "Auf 200 % vergrößern", "Bild vergrößern"),
                    (Icon::Windows, "Wiederholen...", "Bild kacheln"),
                    (Icon::Refit, "Zuschneiden", "Bild zuschneiden"),
                    (Icon::Regenerate, "90° im Uhrzeigersinn", "Im Uhrzeigersinn drehen"),
                    (Icon::Undo, "90° gegen den Uhrzeigersinn", "Gegen den Uhrzeigersinn drehen"),
                    (Icon::Mirror, "Horizontal spiegeln", "Horizontal spiegeln"),
                    (Icon::Up, "Vertikal spiegeln", "Vertikal spiegeln"),
                    (Icon::FullScreen, "Grauskala", "Grauskala"),
                ] {
                    if tool(ui, ic, d, tip).clicked() {
                        hit = Some(cmd);
                    }
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(4.0);
                let (r, resp) = ui.allocate_exact_size(vec2(631.0, 356.0), Sense::hover());
                ui.painter().rect_filled(r, 0.0, Color32::BLACK);
                if let (Some(tex), Some(im)) = (&iu.tex, &iu.img) {
                    let (w, h) = (im.width() as f32, im.height() as f32);
                    let k = (r.width() / w).min(r.height() / h).min(1.0);
                    let ir = Rect::from_center_size(r.center(), vec2(w * k, h * k));
                    if iu.alpha {
                        for (i, y) in (0..).zip((0..(ir.height() / 10.0) as usize + 1).map(|y| ir.top() + y as f32 * 10.0)) {
                            for (j, x) in (0..).zip((0..(ir.width() / 10.0) as usize + 1).map(|x| ir.left() + x as f32 * 10.0)) {
                                if (i + j) % 2 == 0 {
                                    ui.painter().with_clip_rect(ir).rect_filled(Rect::from_min_size(pos2(x, y), vec2(10.0, 10.0)), 0.0, Color32::from_gray(70));
                                }
                            }
                        }
                    }
                    ui.painter().image(tex.id(), ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    if let Some(p) = resp.hover_pos().filter(|p| ir.contains(*p)) {
                        let (x, y) = ((((p.x - ir.left()) / k) as u32).min(im.width() - 1), (((p.y - ir.top()) / k) as u32).min(im.height() - 1));
                        if iu.cursor {
                            status += &format!("X {}  Y {}   ", x, y);
                        }
                        if iu.argb {
                            let px = im.get_pixel(x, y).0;
                            status += &format!("A {}  R {}  G {}  B {}", px[3], px[0], px[1], px[2]);
                        }
                    }
                }
                let (sb, _) = ui.allocate_exact_size(vec2(20.0, 356.0), Sense::hover());
                ui.painter().rect_filled(sb, 0.0, Color32::from_rgb(244, 245, 246));
                ui.painter().rect_stroke(sb, 0.0, Stroke::new(1.0_f32, Color32::from_rgb(226, 228, 230)), StrokeKind::Inside);
            });
            let (hb, _) = ui.allocate_exact_size(vec2(635.0, 22.0), Sense::hover());
            ui.painter().rect_stroke(hb.shrink2(vec2(4.0, 1.0)), 0.0, Stroke::new(1.0_f32, Color32::from_rgb(226, 228, 230)), StrokeKind::Inside);
            let (sr, _) = ui.allocate_exact_size(vec2(635.0, 18.0), Sense::hover());
            let info = match &iu.img {
                Some(im) if status.is_empty() => format!("{} × {} Pixel", im.width(), im.height()),
                _ => status.clone(),
            };
            ui.painter().text(sr.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, info, egui::FontId::proportional(12.5), TEXT);
        });
        let mut msg: Option<(String, MsgKind)> = None;
        if let Some(h) = hit {
            let before = iu.img.clone();
            fn edit(iu: &mut ImageUi, f: &dyn Fn(&image::RgbaImage) -> image::RgbaImage) {
                if let Some(im) = iu.img.take() {
                    iu.img = Some(f(&im));
                    iu.undo = Some(im);
                    iu.stale = true;
                }
            }
            match h {
                "Schließen" => t.image = false,
                "Neu" => {
                    iu.undo = iu.img.take();
                    iu.img = Some(image::RgbaImage::from_pixel(640, 480, image::Rgba([255, 255, 255, 255])));
                    iu.path = None;
                    iu.stale = true;
                }
                "Öffnen..." | "Neu laden" => {
                    let p = if h == "Neu laden" {
                        iu.path.clone()
                    } else {
                        rfd::FileDialog::new().set_title("Bild öffnen").set_directory(self.work_dir()).add_filter("PNG-Bild", &["png"]).pick_file()
                    };
                    if let Some(p) = p {
                        match image::open(&p) {
                            Ok(im) => {
                                iu.undo = iu.img.take();
                                iu.img = Some(im.to_rgba8());
                                iu.path = Some(p);
                                iu.stale = true;
                            }
                            Err(e) => msg = Some((format!("Bild konnte nicht geöffnet werden: {}", e), MsgKind::Error)),
                        }
                    }
                }
                "Speichern" | "Speichern als..." => {
                    let p = if h == "Speichern" {
                        iu.path.clone()
                    } else {
                        rfd::FileDialog::new().set_title("Bild speichern").set_directory(self.work_dir()).set_file_name("bild.png").add_filter("PNG-Bild", &["png"]).save_file()
                    };
                    if let (Some(p), Some(im)) = (p, &iu.img) {
                        match im.save(&p) {
                            Ok(_) => {
                                msg = Some((format!("Bild in {} gespeichert.", p.display()), MsgKind::Info));
                                iu.path = Some(p);
                            }
                            Err(e) => msg = Some((format!("Bild konnte nicht gespeichert werden: {}", e), MsgKind::Error)),
                        }
                    }
                }
                "Widerrufen" => {
                    if let Some(u) = iu.undo.take() {
                        iu.img = Some(u);
                        iu.stale = true;
                    }
                }
                "Kopieren" | "Ausschneiden" => {
                    if let Some(im) = &iu.img {
                        ctx.copy_image(egui::ColorImage::from_rgba_unmultiplied([im.width() as usize, im.height() as usize], im.as_raw()));
                    }
                    if h == "Ausschneiden" {
                        edit(iu, &|im| image::RgbaImage::from_pixel(im.width(), im.height(), image::Rgba([255, 255, 255, 255])));
                    }
                }
                "Alphakanal anzeigen" => iu.alpha = !iu.alpha,
                "Cursor-Position" => iu.cursor = !iu.cursor,
                "ARGB-Wert des Pixels" => iu.argb = !iu.argb,
                "Auf 50 % verkleinern" => edit(iu, &|im| imageops::resize(im, (im.width() / 2).max(1), (im.height() / 2).max(1), imageops::FilterType::Triangle)),
                "Auf 200 % vergrößern" => edit(iu, &|im| imageops::resize(im, (im.width() * 2).min(8192), (im.height() * 2).min(8192), imageops::FilterType::Triangle)),
                "Horizontal spiegeln" => edit(iu, &|im| imageops::flip_horizontal(im)),
                "Vertikal spiegeln" => edit(iu, &|im| imageops::flip_vertical(im)),
                "90° im Uhrzeigersinn" => edit(iu, &|im| imageops::rotate90(im)),
                "90° gegen den Uhrzeigersinn" => edit(iu, &|im| imageops::rotate270(im)),
                "180°" => edit(iu, &|im| imageops::rotate180(im)),
                // Zuschneiden removes the border that has the colour of the top-left pixel
                "Zuschneiden" => edit(iu, &|im| {
                    let bg = *im.get_pixel(0, 0);
                    let (mut x0, mut y0, mut x1, mut y1) = (im.width(), im.height(), 0, 0);
                    for (x, y, p) in im.enumerate_pixels() {
                        if *p != bg {
                            x0 = x0.min(x);
                            y0 = y0.min(y);
                            x1 = x1.max(x);
                            y1 = y1.max(y);
                        }
                    }
                    if x1 < x0 || y1 < y0 {
                        im.clone()
                    } else {
                        imageops::crop_imm(im, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image()
                    }
                }),
                "Wiederholen..." => edit(iu, &|im| {
                    let mut out = image::RgbaImage::new((im.width() * 2).min(8192), (im.height() * 2).min(8192));
                    imageops::tile(&mut out, im);
                    out
                }),
                "Unscharf machen..." => edit(iu, &|im| imageops::blur(im, 1.6)),
                "Verwischen..." => edit(iu, &|im| imageops::blur(im, 4.0)),
                "Schärfen..." => edit(iu, &|im| imageops::unsharpen(im, 1.5, 4)),
                "Aufhellen..." => edit(iu, &|im| imageops::brighten(im, 24)),
                "Kontrast..." => edit(iu, &|im| imageops::contrast(im, 20.0)),
                "Tönung..." => edit(iu, &|im| imageops::huerotate(im, 30)),
                "Sättigung..." => edit(iu, &|im| {
                    let mut o = im.clone();
                    for p in o.pixels_mut() {
                        let g = 0.3 * p.0[0] as f32 + 0.59 * p.0[1] as f32 + 0.11 * p.0[2] as f32;
                        for c in 0..3 {
                            p.0[c] = (g + (p.0[c] as f32 - g) * 1.35).clamp(0.0, 255.0) as u8;
                        }
                    }
                    o
                }),
                "Grauskala" => edit(iu, &|im| {
                    let mut o = im.clone();
                    for p in o.pixels_mut() {
                        let g = (0.3 * p.0[0] as f32 + 0.59 * p.0[1] as f32 + 0.11 * p.0[2] as f32) as u8;
                        p.0[0] = g;
                        p.0[1] = g;
                        p.0[2] = g;
                    }
                    o
                }),
                "Prägen" => edit(iu, &|im| imageops::filter3x3(im, &[-2.0, -1.0, 0.0, -1.0, 1.0, 1.0, 0.0, 1.0, 2.0])),
                "Alphakanal erzeugen..." => edit(iu, &|im| {
                    let bg = *im.get_pixel(0, 0);
                    let mut o = im.clone();
                    for p in o.pixels_mut() {
                        if p.0[..3] == bg.0[..3] {
                            p.0[3] = 0;
                        }
                    }
                    o
                }),
                // an oil-paint look: smooth the picture and reduce it to few colour steps
                "Mit Ölfarbe malen..." if d => edit(iu, &|im| {
                    let mut o = imageops::blur(im, 2.0);
                    for p in o.pixels_mut() {
                        for c in 0..3 {
                            p.0[c] = (p.0[c] / 32) * 32 + 16;
                        }
                    }
                    o
                }),
                "Mit Ölfarbe malen..." | "Dia-Show..." if !d => msg = Some(("Zuerst mit Datei ▸ Öffnen ein Bild laden.".into(), MsgKind::Warning)),
                "Dia-Show..." => {
                    iu.alpha = false;
                    msg = Some(("Dia-Show: Es ist nur ein Bild geladen.".into(), MsgKind::Info));
                }
                _ => {}
            }
            if iu.img != before && iu.img.is_none() {
                iu.tex = None;
            }
        }
        if let Some((m, k)) = msg {
            self.say(m, k);
        }
        if !open {
            t.image = false;
        }
    }

    pub(super) fn dlg_hatch(&mut self, ctx: &egui::Context, t: &mut ToolsUi) {
        if !t.hatch {
            return;
        }
        let h = &mut t.hatch_ui;
        let mut close = false;
        let mut file: Option<&str> = None;
        let rgb = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
        let open = cwin(ctx, "dlg_hatch", "Schraffur-Designer", false, pos2(560.0, 86.0), 497.0, |ui| {
            pad(ui, 12.0, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.add_space(4.0);
                    for (ic, tip) in [
                        (Icon::New, "Neue Schraffur erzeugen."),
                        (Icon::Open, "Schraffurdatei öffnen."),
                        (Icon::Save, "Schraffur speichern."),
                        (Icon::SaveAs, "Schraffur unter anderem Namen speichern."),
                    ] {
                        if tool(ui, ic, true, tip).clicked() {
                            file = Some(match ic {
                                Icon::New => "neu",
                                Icon::Open => "öffnen",
                                _ => "speichern",
                            });
                        }
                    }
                    let (s, _) = ui.allocate_exact_size(vec2(3.0, 30.0), Sense::hover());
                    ui.painter().line_segment([s.center_top(), s.center_bottom()], Stroke::new(1.0_f32, LINE));
                    if tool(ui, Icon::TlDoubleSize, true, "Mustergröße verdoppeln.").clicked() {
                        h.scale *= 2.0;
                    }
                    if tool(ui, Icon::TlHalfSize, true, "Mustergröße halbieren.").clicked() {
                        h.scale *= 0.5;
                    }
                    ui.add_space(4.0);
                    let (r, pen) = ui.allocate_exact_size(vec2(58.0, 34.0), Sense::click());
                    ui.painter().rect_filled(r, 0.0, Color32::from_rgb(250, 250, 250));
                    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
                    icons::paint(ui.painter(), Rect::from_center_size(r.left_center() + vec2(20.0, -3.0), vec2(18.0, 18.0)), Icon::TlPen, true);
                    ui.painter().rect_filled(Rect::from_min_size(r.left_bottom() + vec2(10.0, -9.0), vec2(22.0, 4.0)), 0.0, rgb(DESIGN[h.color]));
                    ui.painter().line_segment([r.left_top() + vec2(40.0, 1.0), r.left_bottom() + vec2(40.0, -1.0)], Stroke::new(1.0_f32, LINE));
                    super::ribbon::tri(ui.painter(), r.right_center() - vec2(9.0, -1.0), TEXT);
                    let id = ui.id().with("hatch_color");
                    if pen.clicked() {
                        ui.memory_mut(|m| m.toggle_popup(id));
                    }
                    super::ribbon::demo_popup(ui, id, "drop:hatch_color");
                    egui::popup_below_widget(ui, id, &pen, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                        ui.set_min_width(286.0);
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(22.0, 24.0), Sense::hover());
                            ui.painter().rect_filled(Rect::from_center_size(r.center(), vec2(15.0, 15.0)), 0.0, rgb(DESIGN[12]));
                            if ui.selectable_label(false, egui::RichText::new("Standard verwenden").size(F).color(TEXT)).clicked() {
                                h.color = 12;
                            }
                        });
                        ui.separator();
                        ui.label(egui::RichText::new("Design-Farben").size(F).color(DIM_TEXT).strong());
                        for row in 0..4 {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 13.0;
                                for c in 0..10 {
                                    let i = row * 10 + c;
                                    let (r, resp) = ui.allocate_exact_size(vec2(15.0, 20.0), Sense::click());
                                    let b = Rect::from_center_size(r.center(), vec2(14.0, 14.0));
                                    ui.painter().rect_filled(b, 0.0, rgb(DESIGN[i]));
                                    ui.painter().rect_stroke(
                                        b,
                                        0.0,
                                        Stroke::new(
                                            if h.color == i { 2.0_f32 } else { 1.0 },
                                            if h.color == i { Color32::from_rgb(230, 40, 30) } else { Color32::from_rgb(90, 96, 104) },
                                        ),
                                        StrokeKind::Inside,
                                    );
                                    if resp.clicked() {
                                        h.color = i;
                                    }
                                }
                            });
                        }
                        ui.add_space(22.0);
                        ui.separator();
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(30.0, 26.0), Sense::hover());
                            icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(20.0, 20.0)), Icon::Appearance, true);
                            ui.label(egui::RichText::new("Weitere Farben...").size(F).color(TEXT));
                        });
                    });
                });
                hline(ui);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if tool(ui, Icon::TlPlus, true, "Neues Muster hinzufügen.").clicked() {
                        file = Some("neu");
                    }
                    tool(ui, Icon::TlMinus, false, "");
                    tool(ui, Icon::Undo, false, "");
                    lbl(ui, "1/1");
                    tool(ui, Icon::Redo, false, "");
                });
                let (r, prev) = ui.allocate_exact_size(vec2(ui.available_width(), 262.0), Sense::hover());
                let p = ui.painter().with_clip_rect(r);
                gradient(&p, r, Color32::from_rgb(72, 96, 124), Color32::from_rgb(156, 196, 226));
                let ang = h.angle.trim().replace(',', ".").parse::<f32>().unwrap_or(45.0).to_radians();
                let dy = h.dy.trim().replace(',', ".").parse::<f32>().unwrap_or(1.0).abs().max(0.02);
                let step = (104.0 * dy * h.scale).clamp(3.0, 2000.0);
                let (dir, nrm) = (vec2(ang.cos(), -ang.sin()), vec2(ang.sin(), ang.cos()));
                let n = (700.0 / step) as i32 + 1;
                for k in -n..=n {
                    let c = r.center() + nrm * (k as f32 * step + step * 0.36);
                    let st = Stroke::new(1.0_f32, rgb(DESIGN[h.color]));
                    let (a, b) = (c - dir * 700.0, c + dir * 700.0);
                    match h.style {
                        0 => {
                            p.line_segment([a, b], st);
                        }
                        1 => {
                            p.extend(egui::Shape::dashed_line(&[a, b], st, 10.0, 6.0));
                        }
                        4 => {
                            p.extend(egui::Shape::dotted_line(&[a, b], st.color, 4.0, 0.8));
                        }
                        _ => {
                            p.extend(egui::Shape::dashed_line(&[a, b], st, 14.0, 4.0));
                        }
                    }
                }
                prev.on_hover_text("Vorschau der Schraffur Hatch0001.");
                ui.horizontal(|ui| {
                    lbl(ui, "Bibliotheks-Schriftarten:");
                    combo(
                        ui,
                        "hatch_font",
                        186.0,
                        &mut h.style,
                        &["————————————", "– – – – – – – – – –", "–·–·–·–·–·–·–·–", "–··–··–··–··–··–", "·····················"],
                        true,
                        None,
                    );
                });
                ui.add_space(6.0);
                lbl_bold(ui, "Linienmerkmale");
                let row = |ui: &mut egui::Ui, a: &str, av: &mut String, b: Option<(&str, &mut String, bool)>| {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(88.0, 32.0), Sense::hover());
                        ui.painter().text(r.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, a, font(), TEXT);
                        field(ui, av, 104.0, true);
                        if let Some((bl, bv, en)) = b {
                            let (r, _) = ui.allocate_exact_size(vec2(84.0, 32.0), Sense::hover());
                            ui.painter().text(r.left_center() + vec2(34.0, 0.0), Align2::LEFT_CENTER, bl, font(), TEXT);
                            field(ui, bv, 104.0, en);
                        }
                    });
                };
                let mut dx = "0.000".to_string();
                row(ui, "Winkel:", &mut h.angle, Some(("Delta X:", &mut dx, false)));
                row(ui, "X-Ursprung:", &mut h.x, Some(("Delta Y:", &mut h.dy, true)));
                row(ui, "Y-Ursprung:", &mut h.y, None);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.spacing_mut().item_spacing.x = 7.0;
                    for k in 1..=6 {
                        ui.vertical(|ui| {
                            white_box(ui, 62.0, 34.0, Color32::WHITE);
                            lbl(ui, &format!("Strich {}", k));
                        });
                    }
                });
            });
            hline(ui);
            pad(ui, 10.0, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if btn(ui, "Schließen", 80.0, 0) {
                            close = true;
                        }
                    });
                });
            });
        });
        match file {
            Some("neu") => *h = HatchUi::default(),
            Some("speichern") => {
                let s = format!("winkel={}\nx={}\ny={}\ndelta_y={}\nskalierung={}\nfarbe={}\nlinie={}\n", h.angle, h.x, h.y, h.dy, h.scale, h.color, h.style);
                self.save_text("Schraffur speichern", "hatch0001.pat", &s);
            }
            Some("öffnen") => {
                if let Some(s) = self.load_text("Schraffur öffnen") {
                    for l in s.lines() {
                        if let Some((k, v)) = l.split_once('=') {
                            let v = v.trim();
                            match k.trim() {
                                "winkel" => h.angle = v.into(),
                                "x" => h.x = v.into(),
                                "y" => h.y = v.into(),
                                "delta_y" => h.dy = v.into(),
                                "skalierung" => h.scale = v.parse().unwrap_or(1.0),
                                "farbe" => h.color = v.parse().unwrap_or(12).min(39),
                                "linie" => h.style = v.parse().unwrap_or(0).min(4),
                                _ => {}
                            }
                        }
                    }
                    self.say("Schraffur geladen.", MsgKind::Info);
                }
            }
            _ => {}
        }
        if close || !open {
            t.hatch = false;
        }
    }

    /// Farbeffekte-Manager: appearances of the model.
    pub(super) fn dlg_appearance(&mut self, ctx: &egui::Context, t: &mut ToolsUi) {
        if !self.windows.appearance {
            return;
        }
        let fu = &mut t.farb;
        if fu.my.is_empty() {
            fu.my = MY_COLORS.to_vec();
        }
        let my = fu.my.clone();
        let mut a = self.data.appearance;
        let rgb = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
        let model_col = Color32::from_rgb((a.color[0] * 255.0) as u8, (a.color[1] * 255.0) as u8, (a.color[2] * 255.0) as u8);
        let (mut close, mut hit) = (false, None);
        let open = cwin(ctx, "dlg_farb", "Farbeffekte-Manager", false, pos2(300.0, 60.0), 747.0, |ui| {
            pad(ui, 12.0, |ui| {
                ui.add_space(8.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(330.0);
                        ui.horizontal(|ui| {
                            ui.add_sized(vec2(120.0, 32.0), egui::TextEdit::singleline(&mut fu.search).hint_text("Suchen...").margin(vec2(8.0, 7.0)));
                            tool(ui, Icon::Search, true, "Farbeffekte suchen");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let r = tiny_drop(ui, Icon::Appearance);
                                if let Some(h) = drop_rows(
                                    ui,
                                    "farb_view",
                                    &r,
                                    &[
                                        M::Radio("Kleine Miniaturansichten", fu.view == 0),
                                        M::Radio("Große Miniaturansichten", fu.view == 1),
                                        M::Radio("Namen und Miniaturansichten", fu.view == 2),
                                        M::Radio("Nur Namen", fu.view == 3),
                                        M::Sep,
                                        M::Chk("Tooltips anzeigen", !fu.tips, true),
                                    ],
                                ) {
                                    match h {
                                        "Kleine Miniaturansichten" => fu.view = 0,
                                        "Große Miniaturansichten" => fu.view = 1,
                                        "Namen und Miniaturansichten" => fu.view = 2,
                                        "Nur Namen" => fu.view = 3,
                                        _ => fu.tips = !fu.tips,
                                    }
                                }
                            });
                        });
                        let section = |ui: &mut egui::Ui, title: &str, right: &dyn Fn(&mut egui::Ui)| {
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(vec2(14.0, 22.0), Sense::hover());
                                super::ribbon::tri(ui.painter(), r.center(), TEXT);
                                lbl(ui, title);
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    right(ui);
                                    let (l, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                                    ui.painter().rect_filled(l, 0.0, LINE);
                                });
                            });
                        };
                        let big = if fu.view == 1 { 22.0 } else { 15.0 };
                        let grid = |ui: &mut egui::Ui, cols: &[[u8; 3]], sel: Option<usize>, key: &str| -> Option<usize> {
                            let mut pick = None;
                            for (row, chunk) in cols.chunks(5).enumerate() {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 3.0;
                                    for (c, col) in chunk.iter().enumerate() {
                                        let i = row * 5 + c;
                                        let (r, resp) = ui.allocate_exact_size(vec2(big * 2.0 + 12.0, big * 2.0 + 8.0), Sense::click());
                                        if sel == Some(i) {
                                            ui.painter().rect_filled(r, 0.0, ROW_SEL);
                                        }
                                        ball(ui.painter(), r.center(), big, rgb(*col));
                                        let resp = if fu.tips { resp } else { resp.on_hover_text(format!("{}{}", key, i + 1)) };
                                        if resp.clicked() {
                                            pick = Some(i);
                                        }
                                    }
                                });
                            }
                            pick
                        };
                        let mut file_hit = None;
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(14.0, 22.0), Sense::hover());
                            super::ribbon::tri(ui.painter(), r.center(), TEXT);
                            lbl(ui, "Meine Farbeffekte");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let (r, resp) = ui.allocate_exact_size(vec2(62.0, 26.0), Sense::click());
                                if resp.hovered() {
                                    ui.painter().rect_filled(r, 0.0, Color32::from_rgb(200, 204, 210));
                                }
                                ui.painter().text(r.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, "Datei", font(), TEXT);
                                super::ribbon::tri(ui.painter(), r.right_center() - vec2(10.0, -1.0), TEXT);
                                file_hit = drop_rows(
                                    ui,
                                    "farb_file",
                                    &resp,
                                    &[
                                        M::Ic(Icon::Open, "Öffnen...", "", true),
                                        M::Ic(Icon::Regenerate, "Anhängen...", "", true),
                                        M::Sep,
                                        M::Ic(Icon::SaveAs, "Speichern als...", "", true),
                                    ],
                                );
                                if tool(ui, Icon::Delete, my.len() > 1, "Gewählten Farbeffekt löschen").clicked() {
                                    file_hit = Some("Löschen");
                                }
                                if tool(ui, Icon::New, true, "Neuen Farbeffekt aus der Modellfarbe erzeugen").clicked() {
                                    file_hit = Some("Neu");
                                }
                                let (l, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                                ui.painter().rect_filled(l, 0.0, LINE);
                            });
                        });
                        if let Some(h) = file_hit {
                            hit = Some(h);
                        }
                        if let Some(i) = grid(ui, &my, Some(fu.sel), "ref_color") {
                            fu.sel = i;
                            let c = my[i];
                            a.color = [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0];
                        }
                        ui.add_space(6.0);
                        section(ui, "Modell", &|ui| {
                            tool(ui, Icon::SaveAs, false, "");
                        });
                        grid(ui, &[[model_col.r(), model_col.g(), model_col.b()]], None, "modell");
                        ui.add_space(14.0);
                        section(ui, "Bibliothek", &|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(112.0, 22.0), Sense::hover());
                            ui.painter().text(r.left_center(), Align2::LEFT_CENTER, "std-metals.dmt", font(), TEXT);
                            super::ribbon::tri(ui.painter(), r.right_center() - vec2(6.0, -1.0), TEXT);
                        });
                        if let Some(i) = grid(ui, &LIB_COLORS, None, "metal") {
                            let c = LIB_COLORS[i];
                            a.color = [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0];
                            a.shininess = 0.8;
                        }
                    });
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            lbl(ui, "Name");
                            let mut n = format!("ref_color{}", fu.sel + 1);
                            field(ui, &mut n, ui.available_width(), false);
                        });
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                            ui.painter().rect_filled(Rect::from_center_size(r.center(), vec2(12.0, 9.0)), 3.0, Color32::from_rgb(60, 66, 74));
                            lbl_bold(ui, "Farbeffekte in der Vorschau anzeigen");
                        });
                        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 190.0), Sense::hover());
                        gradient(ui.painter(), r, Color32::from_rgb(72, 96, 124), Color32::from_rgb(156, 196, 226));
                        ball(ui.painter(), r.center(), 80.0, model_col);
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(84.0, 32.0), Sense::hover());
                            ui.painter().text(r.left_center(), Align2::LEFT_CENTER, "Schlüsselwort", font(), TEXT);
                            field(ui, &mut fu.key, ui.available_width(), true);
                        });
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(84.0, 32.0), Sense::hover());
                            ui.painter().text(r.left_center(), Align2::LEFT_CENTER, "Beschreibung", font(), TEXT);
                            field(ui, &mut fu.desc, ui.available_width(), true);
                        });
                        ui.add_space(6.0);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        tabs(ui, &mut fu.tab, &["Eigenschaften", "Textur", "Bump", "Decal"], 9);
                        tab_page(ui, |ui| {
                            ui.set_min_height(372.0);
                            if fu.tab != 0 {
                                lbl_dim(ui, "Für diesen Farbeffekt ist keine Abbildung zugewiesen.");
                                return;
                            }
                            let mut z = 0;
                            ui.horizontal(|ui| {
                                lbl(ui, "Klasse");
                                combo(ui, "farb_class", 84.0, &mut z, &["Generisch"], false, None);
                                lbl(ui, "Unterklasse");
                                combo(ui, "farb_sub", ui.available_width(), &mut z, &["Keine"], false, None);
                            });
                            ui.add_space(6.0);
                            hline(ui);
                            ui.add_space(6.0);
                            let swatch = |ui: &mut egui::Ui, name: &str, col: Option<&mut [f32; 3]>| {
                                ui.horizontal(|ui| {
                                    let (r, _) = ui.allocate_exact_size(vec2(96.0, 32.0), Sense::hover());
                                    ui.painter().text(r.left_center(), Align2::LEFT_CENTER, name, font(), if col.is_some() { TEXT } else { DIM_TEXT });
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match col {
                                        Some(c) => {
                                            ui.color_edit_button_rgb(c);
                                        }
                                        None => {
                                            let (r, _) = ui.allocate_exact_size(vec2(52.0, 30.0), Sense::hover());
                                            ui.painter().rect_filled(r, 0.0, Color32::from_rgb(234, 236, 238));
                                            ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
                                        }
                                    });
                                });
                            };
                            let slide = |ui: &mut egui::Ui, name: &str, v: &mut f32, en: bool| -> bool {
                                let mut ch = false;
                                ui.horizontal(|ui| {
                                    let (r, _) = ui.allocate_exact_size(vec2(86.0, 32.0), Sense::hover());
                                    ui.painter().text(r.left_center(), Align2::LEFT_CENTER, name, font(), if en { TEXT } else { DIM_TEXT });
                                    ch = cslider(ui, v, 100.0, 170.0, en);
                                    ui.add_space(6.0);
                                    let mut s = format!("{:.2}", v);
                                    field(ui, &mut s, ui.available_width(), false);
                                });
                                ch
                            };
                            swatch(ui, "Farbe", Some(&mut a.color));
                            let (mut i90, mut i60, mut i30) = (90.0f32, 60.0f32, 30.0f32);
                            slide(ui, "Intensität", &mut i90, false);
                            slide(ui, "Raumlicht", &mut i60, false);
                            swatch(ui, "Glanzlichtfarbe", None);
                            let mut gl = a.shininess * 100.0;
                            if slide(ui, "Glanz", &mut gl, true) {
                                a.shininess = (gl / 100.0).clamp(0.0, 1.0);
                            }
                            slide(ui, "Glanzlicht", &mut i60, false);
                            slide(ui, "Reflexion", &mut i30, false);
                            let mut tr = (1.0 - a.opacity) * 100.0;
                            if slide(ui, "Transparenz", &mut tr, true) {
                                a.opacity = (1.0 - tr / 100.0).clamp(0.1, 1.0);
                            }
                        });
                    });
                });
            });
            hline(ui);
            pad(ui, 10.0, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if btn(ui, "Schließen", 84.0, 0) {
                            close = true;
                        }
                    });
                });
            });
        });
        if let Some(h) = hit {
            let fu = &mut t.farb;
            let parse = |s: &str| -> Vec<[u8; 3]> {
                s.lines()
                    .filter_map(|l| {
                        let v: Vec<u8> = l.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                        if v.len() == 3 {
                            Some([v[0], v[1], v[2]])
                        } else {
                            None
                        }
                    })
                    .collect()
            };
            match h {
                "Neu" => {
                    fu.my.push([(a.color[0] * 255.0) as u8, (a.color[1] * 255.0) as u8, (a.color[2] * 255.0) as u8]);
                    fu.sel = fu.my.len() - 1;
                }
                "Löschen" => {
                    if fu.my.len() > 1 && fu.sel < fu.my.len() {
                        fu.my.remove(fu.sel);
                        fu.sel = fu.sel.min(fu.my.len() - 1);
                    }
                }
                "Speichern als..." => {
                    let s = fu.my.iter().map(|c| format!("{} {} {}", c[0], c[1], c[2])).collect::<Vec<_>>().join("\n");
                    self.save_text("Farbeffekte speichern", "farbeffekte.dmt", &s);
                }
                "Öffnen..." | "Anhängen..." => {
                    if let Some(s) = self.load_text("Farbeffektdatei öffnen") {
                        let cols = parse(&s);
                        if cols.is_empty() {
                            self.say("Die Datei enthält keine Farbeffekte.", MsgKind::Warning);
                        } else {
                            let n = cols.len();
                            if h == "Öffnen..." {
                                fu.my = cols;
                                fu.sel = 0;
                            } else {
                                fu.my.extend(cols);
                            }
                            self.say(format!("{} Farbeffekt(e) geladen.", n), MsgKind::Info);
                        }
                    }
                }
                h => self.todo2(h),
            }
        }
        if a != self.data.appearance {
            self.data.appearance = a;
            self.dirty = true;
        }
        if close || !open {
            self.windows.appearance = false;
        }
    }
}

/// Small icon button with a ▾ (the thumbnail-size menu of the Farbeffekte-Manager).
fn tiny_drop(ui: &mut egui::Ui, icon: Icon) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(34.0, 26.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, 0.0, Color32::from_rgb(200, 204, 210));
    }
    icons::paint(ui.painter(), Rect::from_center_size(r.left_center() + vec2(12.0, 0.0), vec2(16.0, 16.0)), icon, true);
    super::ribbon::tri(ui.painter(), r.right_center() - vec2(7.0, -1.0), TEXT);
    resp
}
