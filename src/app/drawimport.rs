//! Opening a 2D drawing (DXF, DWG): the part is built from its views at once, and a dialog
//! shows what was found (views, projection method, scale) and lets those be corrected.

use super::dlgkit::{btn, combo, hline, lbl, lbl_bold, lbl_dim};
use super::*;
use crate::model::drawing3d::{self, Options, Projection, Role};
use eframe::egui;
use std::path::{Path, PathBuf};

/// State of the import dialog.
#[derive(Clone)]
pub struct D2Import {
    pub path: PathBuf,
    /// 0 = from the drawing, 1 = European (first angle), 2 = American (third angle)
    pub projection: usize,
    /// model mm per drawn mm, as text ("2" for 1:2); empty = from the drawing
    pub scale: String,
    /// thickness of a single view (mm)
    pub thickness: String,
    pub single: bool,
    pub found: Vec<String>,
    pub notes: Vec<String>,
}

pub fn is_drawing2d(name: &str) -> bool {
    let n = name.to_lowercase();
    n.ends_with(".dxf") || n.ends_with(".dwg")
}

impl App {
    /// Datei ▸ Öffnen of a DXF or DWG file: a new part built from the drawing's views.
    pub fn open_drawing2d(&mut self, path: &Path) {
        let opt = Options::default();
        self.build_from_drawing(path, &opt, None);
    }

    fn build_from_drawing(&mut self, path: &Path, opt: &Options, keep: Option<D2Import>) {
        match drawing3d::import_file(path, opt) {
            Ok((model, a, notes)) => {
                let name = model.name.clone();
                self.new_part_named(&name);
                self.data = model;
                self.regenerate(false);
                self.undo.clear();
                self.mark_saved();
                self.fit_pending = true;
                let errors = self.result.errors.len();
                let role_name = |r: &Role| match r {
                    Role::Front => "Vorderansicht".to_string(),
                    Role::Plan { below } => format!("Draufsicht ({})", if *below { "unter der Vorderansicht" } else { "über der Vorderansicht" }),
                    Role::Side { right } => format!("Seitenansicht ({})", if *right { "rechts" } else { "links" }),
                };
                let mut found: Vec<String> = a.roles.iter().map(|(_, r)| role_name(r)).collect();
                found.push(format!("Projektionsmethode: {}", if a.projection == Projection::First { "1 – europäisch" } else { "3 – amerikanisch" }));
                found.push(format!("Maßstab: {} ({})", scale_text(a.scale), a.scale_source));
                let single = a.roles.len() == 1;
                let d = match keep {
                    Some(mut k) => {
                        k.found = found;
                        k.notes = notes.clone();
                        k.single = single;
                        k
                    }
                    None => D2Import {
                        path: path.to_path_buf(),
                        projection: 0,
                        scale: String::new(),
                        thickness: a.thickness.map(|t| fmt_num(t, 3)).unwrap_or_else(|| "10".into()),
                        single,
                        found,
                        notes: notes.clone(),
                    },
                };
                self.d2_import = Some(d);
                if errors > 0 {
                    self.say(format!("{}: Modell aufgebaut, {} KE(s) mit Fehlern – Ansichten und Maßstab im Dialog prüfen.", path.display(), errors), MsgKind::Warning);
                } else {
                    self.say(format!("{} als 3D-Teil aufgebaut ({} Ansicht(en), {} KEs).", path.display(), a.roles.len(), self.data.features.len().saturating_sub(4)), MsgKind::Success);
                }
                self.add_recent(&path.to_path_buf());
                if let Some(dir) = path.parent() {
                    self.browser_dir = dir.to_path_buf();
                }
            }
            Err(e) => self.say(format!("Zeichnung nicht übernommen: {}", e), MsgKind::Error),
        }
    }

    /// The "2D-Zeichnung übernehmen" dialog after opening a DXF/DWG file.
    pub fn drawing2d_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.d2_import.clone() else { return };
        let mut rebuild = false;
        let mut close = false;
        let pos = egui::pos2(ctx.screen_rect().width() - 470.0, 150.0);
        let open = super::dlgkit::cwin(ctx, "d2_import", "2D-Zeichnung übernehmen", false, pos, 420.0, |ui| {
            egui::Frame::NONE.inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
                lbl_bold(ui, &d.path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
                ui.add_space(4.0);
                for f in &d.found {
                    lbl(ui, &format!("• {}", f));
                }
                for n in &d.notes {
                    lbl_dim(ui, n);
                }
                ui.add_space(6.0);
                hline(ui);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    lbl(ui, "Projektionsmethode");
                    combo(ui, "d2_proj", 210.0, &mut d.projection, &["aus der Zeichnung", "1 – europäisch (ISO E)", "3 – amerikanisch"], true, None);
                });
                ui.horizontal(|ui| {
                    lbl(ui, "Maßstab 1 :");
                    ui.add(egui::TextEdit::singleline(&mut d.scale).desired_width(70.0).hint_text("auto"));
                    lbl_dim(ui, "leer = aus Bemaßung/Schriftfeld");
                });
                if d.single {
                    ui.horizontal(|ui| {
                        lbl(ui, "Dicke (eine Ansicht)");
                        ui.add(egui::TextEdit::singleline(&mut d.thickness).desired_width(70.0));
                        lbl(ui, "mm");
                    });
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if btn(ui, "Neu aufbauen", 120.0, 0) {
                        rebuild = true;
                    }
                    if btn(ui, "OK", 80.0, 1) {
                        close = true;
                    }
                });
            });
        });
        if !open || close {
            self.d2_import = None;
            return;
        }
        if rebuild {
            let opt = Options {
                projection: match d.projection {
                    1 => Some(Projection::First),
                    2 => Some(Projection::Third),
                    _ => None,
                },
                scale: d.scale.trim().replace(',', ".").parse::<f64>().ok().filter(|s| *s > 0.0),
                thickness: d.thickness.trim().replace(',', ".").parse::<f64>().ok().filter(|t| *t > 0.0),
            };
            let path = d.path.clone();
            self.build_from_drawing(&path, &opt, Some(d));
            return;
        }
        self.d2_import = Some(d);
    }
}

fn scale_text(k: f64) -> String {
    if k >= 1.0 {
        format!("1:{}", fmt_num(k, 3))
    } else {
        format!("{}:1", fmt_num(1.0 / k, 3))
    }
}
