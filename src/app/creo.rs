//! Creo Parametric 10 window pieces that are not about modelling itself: the Startseite
//! (home screen without a model), the extra ribbon tabs (Live-Simulation, Anmerkungen
//! erstellen, Flexible Modellierung, Anwendungen, HTL), the "Datei öffnen" dialog and the
//! Werkzeuge dialogs (Suchen, Referenz-Viewer, Modell-Player).

use super::camera::{Camera, StdView};
use super::icons::{self, Icon};
use super::ribbon::{group, group_drop, group_menu, large, menu_entry, menu_item, menu_row, menu_spacing, small, split, stack, theme::*, tiny};
use super::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, RichText, Sense, Stroke};
use std::path::{Path, PathBuf};

/// Graphics-window background presets (Creo default, HTL "Hintergrund Weiss/Dunkel").
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Background { #[default] Standard, White, Dark }

impl Background {
    pub fn colors(self) -> (Color32, Color32) {
        match self {
            Background::Standard => (Color32::from_rgb(69, 95, 123), Color32::from_rgb(159, 202, 231)),
            Background::White => (Color32::WHITE, Color32::WHITE),
            Background::Dark => (Color32::from_rgb(38, 42, 48), Color32::from_rgb(70, 76, 86)),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpenKind { All, Part, Assembly, Drawing, Import, Step, Mesh, Creo }

/// Creo's own files: name.prt, name.prt.3, name.asm, name.drw ...
pub fn is_creo_file(name: &str) -> bool {
    let n = name.to_lowercase();
    let base = match n.rsplit_once('.') { Some((b, v)) if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) => b.to_string(), _ => n };
    base.ends_with(".prt") || base.ends_with(".asm") || base.ends_with(".drw")
}

/// Creo file of one kind ("prt", "asm", "drw"), with or without version number.
pub fn is_creo_kind(name: &str, kind: &str) -> bool {
    let n = name.to_lowercase();
    let base = match n.rsplit_once('.') { Some((b, v)) if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) => b.to_string(), _ => n };
    base.ends_with(&format!(".{}", kind))
}

pub fn is_neutral_file(name: &str) -> bool {
    let n = name.to_lowercase();
    [".stp", ".step", ".p21", ".stl", ".obj"].iter().any(|e| n.ends_with(e))
}

impl OpenKind {
    pub const ALL: [OpenKind; 8] = [OpenKind::All, OpenKind::Part, OpenKind::Assembly, OpenKind::Drawing, OpenKind::Import, OpenKind::Step, OpenKind::Mesh, OpenKind::Creo];
    fn label(self) -> &'static str {
        match self {
            OpenKind::All => "Alle unterstützten Dateien",
            OpenKind::Part => "Teil (.pfprt, Creo .prt)",
            OpenKind::Assembly => "Baugruppe (.pfasm, Creo .asm)",
            OpenKind::Drawing => "Zeichnung (.pfdrw, Creo .drw)",
            OpenKind::Import => "Importierbar (.stp, .step, .stl, .obj, .prt)",
            OpenKind::Step => "STEP (.stp, .step)",
            OpenKind::Mesh => "Netz (.stl, .obj)",
            OpenKind::Creo => "Creo-Dateien (.prt, .asm, .drw)",
        }
    }
    pub fn accepts(self, name: &str) -> bool {
        let n = name.to_lowercase();
        match self {
            OpenKind::All => n.ends_with(".pfprt") || n.ends_with(".pfasm") || n.ends_with(".pfdrw") || is_neutral_file(&n) || is_creo_file(&n),
            OpenKind::Part => n.ends_with(".pfprt") || is_creo_kind(&n, "prt"),
            OpenKind::Assembly => n.ends_with(".pfasm") || is_creo_kind(&n, "asm"),
            OpenKind::Drawing => n.ends_with(".pfdrw") || is_creo_kind(&n, "drw"),
            OpenKind::Import => is_neutral_file(&n) || is_creo_file(&n),
            OpenKind::Step => n.ends_with(".stp") || n.ends_with(".step") || n.ends_with(".p21"),
            OpenKind::Mesh => n.ends_with(".stl") || n.ends_with(".obj"),
            OpenKind::Creo => is_creo_file(&n),
        }
    }
}

/// What Freo can tell about a native Creo file (its geometry is in PTC's closed format).
pub struct CreoFileInfo {
    pub path: PathBuf,
    pub kind: String,
    pub model: String,
    pub version: String,
    pub machine: String,
    pub thumb: Option<egui::TextureHandle>,
    pub step: Option<PathBuf>,
}

/// Reads the plain-text UGC header and the embedded 256×256 preview of a Creo file.
pub fn read_creo_header(path: &Path) -> Option<(String, String, String, String, Option<egui::ColorImage>)> {
    let d = std::fs::read(path).ok()?;
    if !d.starts_with(b"#UGC") { return None; }
    let head = String::from_utf8_lossy(&d[..d.len().min(4096)]).to_string();
    let kind = head.split_whitespace().nth(1).unwrap_or("").to_string();
    let field = |k: &str| head.lines().find(|l| l.starts_with(&format!("#- {}", k))).map(|l| l[3 + k.len()..].trim_end_matches('\\').trim().to_string()).unwrap_or_default();
    let model = { let m = field("CMNM"); m.get(3..).map(str::to_string).unwrap_or(m) };
    let version = head.lines().find(|l| l.starts_with("#Creo") || l.starts_with("#Pro/ENGINEER")).map(|l| l.split_whitespace().last().unwrap_or("").to_string()).unwrap_or_default();
    let machine = field("MACH").trim_start_matches('_').to_string();
    let thumb = (|| {
        let a = find_bytes(&d, b"\xff\xd8\xff")?;
        let b = find_bytes(&d[a..], b"\xff\xd9")? + a + 2;
        let dir = std::env::temp_dir().join("freo_thumbs");
        std::fs::create_dir_all(&dir).ok()?;
        let (jpg, bmp) = (dir.join("thumb.jpg"), dir.join("thumb.bmp"));
        std::fs::write(&jpg, &d[a..b]).ok()?;
        // macOS sips decodes the JPEG; an uncompressed BMP is trivial to read back
        let ok = std::process::Command::new("sips").args(["-s", "format", "bmp"]).arg(&jpg).arg("--out").arg(&bmp).output().ok()?.status.success();
        if !ok { return None; }
        read_bmp(&std::fs::read(&bmp).ok()?)
    })();
    Some((kind, model, version, machine, thumb))
}

fn find_bytes(h: &[u8], n: &[u8]) -> Option<usize> { h.windows(n.len()).position(|w| w == n) }

fn read_bmp(b: &[u8]) -> Option<egui::ColorImage> {
    if b.len() < 54 || &b[..2] != b"BM" { return None; }
    let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let off = u32_at(10) as usize;
    let w = u32_at(18) as i32 as i64;
    let h = u32_at(22) as i32 as i64;
    let bpp = u16::from_le_bytes([b[28], b[29]]) as usize;
    if w <= 0 || h == 0 || !(bpp == 24 || bpp == 32) { return None; }
    let (w, hh) = (w as usize, h.unsigned_abs() as usize);
    let stride = (w * bpp / 8 + 3) & !3;
    let mut px = vec![egui::Color32::WHITE; w * hh];
    for y in 0..hh {
        let row = if h > 0 { hh - 1 - y } else { y };
        for x in 0..w {
            let o = off + row * stride + x * bpp / 8;
            if o + 2 >= b.len() { return None; }
            px[y * w + x] = egui::Color32::from_rgb(b[o + 2], b[o + 1], b[o]);
        }
    }
    Some(egui::ColorImage { size: [w, hh], pixels: px })
}

pub struct OpenDialog {
    pub dir: PathBuf,
    pub selected: Option<PathBuf>,
    pub name: String,
    pub kind: OpenKind,
    pub search: String,
    pub history: Vec<PathBuf>,
    /// add the file to the current part (Daten abrufen ▸ Importieren) instead of opening it
    pub insert: bool,
    /// Spotlight results shown instead of the folder ("title", files)
    pub found: Option<(String, Vec<PathBuf>)>,
    pub pending: Option<(String, std::sync::mpsc::Receiver<Vec<PathBuf>>)>,
}

/// What "Auf diesem Mac suchen" looks for.
pub enum SpotQuery { Name(String), Models }

/// Paths that are never interesting results (caches, build output, Trash, temp).
fn junk_path(p: &Path) -> bool {
    let s = p.to_string_lossy();
    ["/Library/", "/node_modules/", "/.Trash/", "/target/", "/private/", "/System/", "/.git/"].iter().any(|j| s.contains(j))
}

/// Model files below `root` (for volumes Spotlight does not index, e.g. some SD cards).
fn walk_models(root: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth == 0 || out.len() > 2000 { return; }
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let n = e.file_name().to_string_lossy().to_string();
        if n.starts_with('.') { continue; }
        if p.is_dir() { walk_models(&p, depth - 1, out); } else if OpenKind::All.accepts(&n) { out.push(p); }
    }
}

/// External drives (SD card, USB stick) mounted under /Volumes.
pub fn volumes() -> Vec<PathBuf> {
    std::fs::read_dir("/Volumes").map(|rd| rd.flatten().map(|e| e.path()).filter(|p| {
        let n = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        // the system disk shows up as a link to "/"
        !n.starts_with('.') && n != "Macintosh HD" && n != "Recovery" && std::fs::read_link(p).is_err() && p.is_dir()
    }).collect()).unwrap_or_default()
}

/// Runs a Spotlight search (plus a scan of external drives) in the background.
pub fn spotlight(q: SpotQuery, kind: OpenKind) -> std::sync::mpsc::Receiver<Vec<PathBuf>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut found: Vec<PathBuf> = vec![];
        let args: Vec<String> = match &q {
            SpotQuery::Name(t) => vec!["-name".into(), t.clone()],
            SpotQuery::Models => {
                let pats = ["*.prt", "*.prt.*", "*.asm", "*.asm.*", "*.drw", "*.drw.*", "*.stp", "*.step", "*.pfprt", "*.pfasm", "*.pfdrw"];
                vec![pats.iter().map(|p| format!("kMDItemFSName == \"{}\"c", p)).collect::<Vec<_>>().join(" || ")]
            }
        };
        if let Ok(o) = std::process::Command::new("mdfind").args(&args).output() {
            found.extend(String::from_utf8_lossy(&o.stdout).lines().map(PathBuf::from));
        }
        if matches!(q, SpotQuery::Models) { for v in volumes() { walk_models(&v, 6, &mut found); } }
        let needle = match &q { SpotQuery::Name(t) => t.to_lowercase(), SpotQuery::Models => String::new() };
        let mut out: Vec<(std::time::SystemTime, PathBuf)> = found.into_iter().filter(|p| {
            let n = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            p.is_file() && !junk_path(p) && kind.accepts(&n) && (needle.is_empty() || n.to_lowercase().contains(&needle))
        }).map(|p| (std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), p)).collect();
        out.sort_by(|a, b| b.0.cmp(&a.0));
        out.dedup_by(|a, b| a.1 == b.1);
        let _ = tx.send(out.into_iter().take(400).map(|x| x.1).collect());
    });
    rx
}

/// Datei ▸ Speichern unter ▸ Kopie speichern: target folder, new name and type.
pub struct ExportDialog {
    pub dir: PathBuf,
    pub name: String,
    /// None = native Freo part copy
    pub format: Option<super::export::Format>,
    pub history: Vec<PathBuf>,
    pub selected: Option<PathBuf>,
}

/// What a right-click on a folder list asked for (run after the list is drawn).
#[derive(Clone, Debug)]
pub enum FsAction {
    NewFolder(PathBuf), Rename(PathBuf), Reveal(PathBuf), Enter(PathBuf),
    Copy(PathBuf), Cut(PathBuf), Paste(PathBuf), Duplicate(PathBuf), Trash(PathBuf), PutBack, CopyPath(PathBuf),
}

/// "Neuer Ordner" / "Umbenennen" name prompt.
pub struct NameDialog { pub parent: PathBuf, pub name: String, pub rename: Option<PathBuf>, pub error: Option<String>, pub focus: bool }

/// An annotation plane of the Anmerkungen-erstellen tab.
#[derive(Clone)]
pub struct AnnotPlane {
    pub name: String,
    /// orientation the plane faces; None for FLAT TO SCREEN
    pub rot: Option<glam::DQuat>,
    pub icon: Icon,
    /// Galeriedarstellung: shown in the ribbon gallery
    pub gallery: bool,
    /// one of Creo's seven standard planes (cannot be renamed or deleted)
    pub builtin: bool,
}

/// Annotation planes plus the gallery and Anmerkungsebenen-Manager state.
pub struct AnnotPlanes {
    pub list: Vec<AnnotPlane>,
    pub active: usize,
    /// row of the gallery shown in the ribbon
    pub row: usize,
    /// gallery folded out to show every plane
    pub gallery_open: bool,
    /// Anmerkungsebenen-Manager open: (selected row, name being edited)
    pub manager: Option<(usize, Option<String>)>,
    /// Modell bei Festlegung neu orientieren
    pub reorient: bool,
    /// Ebenenraster anzeigen / Rasterabstand
    pub grid: bool,
    pub grid_step: f64,
    pub grid_text: String,
}

impl Default for AnnotPlanes {
    fn default() -> Self {
        let std = [("FLAT TO SCREEN", None, Icon::AnnotFlat), ("FRONT", Some(StdView::Front), Icon::AnnotFront), ("TOP", Some(StdView::Top), Icon::AnnotTop),
            ("RIGHT", Some(StdView::Right), Icon::AnnotRight), ("BACK", Some(StdView::Back), Icon::AnnotBack), ("BOTTOM", Some(StdView::Bottom), Icon::AnnotBottom),
            ("LEFT", Some(StdView::Left), Icon::AnnotLeft)];
        let list = std.into_iter().map(|(n, v, icon)| AnnotPlane { name: n.into(), rot: v.map(|v| v.rotation()), icon, gallery: true, builtin: true }).collect();
        AnnotPlanes { list, active: 0, row: 0, gallery_open: false, manager: None, reorient: false, grid: true, grid_step: 0.1, grid_text: format!("{:.6}", 0.1) }
    }
}

const GALLERY_COLS: usize = 4;
const TILE_W: f32 = 84.0;

/// Creo's check box: white square with a dark tick, label to the right. Returns true when toggled.
pub(super) fn creo_check(ui: &mut egui::Ui, value: &mut bool, label: &str) -> bool {
    let font = FontId::proportional(14.0);
    let w = ui.fonts(|fo| fo.layout_no_wrap(label.to_string(), font.clone(), TEXT).size().x);
    let (r, resp) = ui.allocate_exact_size(vec2(w + 34.0, 24.0), Sense::click());
    let b = Rect::from_center_size(pos2(r.left() + 10.0, r.center().y), vec2(15.0, 15.0));
    check_box(ui.painter(), b, *value);
    ui.painter().text(pos2(r.left() + 30.0, r.center().y), Align2::LEFT_CENTER, label, font, TEXT);
    if resp.clicked() { *value = !*value; }
    resp.clicked()
}

pub(super) fn check_box(p: &egui::Painter, b: Rect, on: bool) {
    p.rect_filled(b, 1.0, Color32::WHITE);
    p.rect_stroke(b, 1.0, Stroke::new(1.0_f32, Color32::from_rgb(90, 96, 104)), egui::StrokeKind::Inside);
    if on { p.add(egui::Shape::line(vec![b.left_center() + vec2(3.0, 0.0), b.center_bottom() + vec2(-1.0, -3.5), b.right_top() + vec2(-3.0, 3.0)], Stroke::new(1.8_f32, Color32::from_rgb(40, 44, 50)))); }
}

/// Creo's titled group box (a frame with its caption set into the top border).
pub(super) fn fieldset(ui: &mut egui::Ui, title: &str, f: impl FnOnce(&mut egui::Ui)) {
    let top = ui.cursor().top();
    let inner = egui::Frame::NONE.inner_margin(egui::Margin { left: 10, right: 10, top: 22, bottom: 10 }).show(ui, |ui| { ui.set_width(ui.available_width()); f(ui) });
    let r = Rect::from_min_max(pos2(inner.response.rect.left(), top + 8.0), inner.response.rect.right_bottom());
    let p = ui.painter();
    p.rect_stroke(r, 2.0, Stroke::new(1.0_f32, Color32::from_rgb(204, 208, 214)), egui::StrokeKind::Inside);
    let font = FontId::proportional(14.0);
    let w = ui.fonts(|fo| fo.layout_no_wrap(title.to_string(), font.clone(), TEXT).size().x);
    p.rect_filled(Rect::from_min_size(pos2(r.left() + 12.0, top), vec2(w + 8.0, 16.0)), 0.0, ui.visuals().window_fill);
    p.text(pos2(r.left() + 16.0, top + 8.0), Align2::LEFT_CENTER, title, font, TEXT);
}

/// Clipboard and undo state the right-click menu shows.
#[derive(Clone, Default)]
pub struct FsCtx { pub clip: Option<(PathBuf, bool)>, pub trashed: Option<(PathBuf, PathBuf)> }

fn short(p: &Path) -> String { p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default() }

/// Right-click menu for folder lists: `dir` is the folder shown, `item` the entry clicked (if any).
pub fn fs_menu(ui: &mut egui::Ui, dir: &Path, item: Option<&Path>, fx: &FsCtx, out: &mut Option<FsAction>) {
    menu_spacing(ui);
    let mut act = |ui: &mut egui::Ui, icon: Icon, label: &str, enabled: bool, a: FsAction| {
        if menu_entry(ui, icon, label, enabled, false).clicked() { *out = Some(a); ui.close_menu(); }
    };
    if let Some(p) = item {
        act(ui, if p.is_dir() { Icon::FolderItem } else { Icon::Open }, "Öffnen", true, FsAction::Enter(p.to_path_buf()));
        ui.separator();
    }
    if let Some(d) = item.filter(|p| p.is_dir()) { act(ui, Icon::FolderItem, &format!("Neuer Ordner in „{}“", short(d)), true, FsAction::NewFolder(d.to_path_buf())); }
    act(ui, Icon::FolderItem, "Neuer Ordner", dir.is_dir(), FsAction::NewFolder(dir.to_path_buf()));
    ui.separator();
    if let Some(p) = item {
        act(ui, Icon::Copy, "Kopieren", true, FsAction::Copy(p.to_path_buf()));
        act(ui, Icon::Trim, "Ausschneiden", true, FsAction::Cut(p.to_path_buf()));
    }
    let target = item.filter(|p| p.is_dir()).unwrap_or(dir);
    let paste = match &fx.clip { Some((c, cut)) => format!("„{}“ {}", short(c), if *cut { "hierher verschieben" } else { "einfügen" }), None => "Einfügen".into() };
    act(ui, Icon::Paste, &paste, fx.clip.as_ref().map(|c| c.0.exists() && !target.starts_with(&c.0)).unwrap_or(false), FsAction::Paste(target.to_path_buf()));
    if let Some(p) = item {
        act(ui, Icon::Copy, "Duplizieren", true, FsAction::Duplicate(p.to_path_buf()));
        act(ui, Icon::SkModify, "Umbenennen", true, FsAction::Rename(p.to_path_buf()));
        ui.separator();
        act(ui, Icon::Delete, "In den Papierkorb legen   ⌘⌫", true, FsAction::Trash(p.to_path_buf()));
    }
    if let Some((orig, _)) = &fx.trashed { act(ui, Icon::Undo, &format!("„{}“ zurücklegen", short(orig)), true, FsAction::PutBack); }
    ui.separator();
    act(ui, Icon::Parameters, "Pfad kopieren", true, FsAction::CopyPath(item.unwrap_or(dir).to_path_buf()));
    act(ui, Icon::Open, "Im Finder zeigen", true, FsAction::Reveal(item.unwrap_or(dir).to_path_buf()));
}

/// "name Kopie.ext", "name Kopie 2.ext", … — the first one free in `dir`.
pub fn copy_name(dir: &Path, src: &Path) -> PathBuf {
    let file = short(src);
    let (stem, ext) = if src.is_dir() { (file.clone(), String::new()) } else { match file.rsplit_once('.') { Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{}", e)), _ => (file.clone(), String::new()) } };
    if !dir.join(&file).exists() { return dir.join(file); }
    (1..).map(|i| dir.join(if i == 1 { format!("{} Kopie{}", stem, ext) } else { format!("{} Kopie {}{}", stem, i, ext) })).find(|p| !p.exists()).unwrap()
}

pub fn copy_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir(dst)?;
        for e in std::fs::read_dir(src)? { let e = e?; copy_recursive(&e.path(), &dst.join(e.file_name()))?; }
        Ok(())
    } else { std::fs::copy(src, dst).map(|_| ()) }
}

/// Title bar of Freo's dialogs, drawn here (instead of egui's) so the close × can turn red,
/// get a red outline and grow while the pointer is on it. Returns true when × was clicked.
pub fn dialog_header(ui: &mut egui::Ui, title: &str) -> bool {
    const BAR: f32 = 30.0;
    let m = ui.style().spacing.window_margin;
    let inner = ui.max_rect();
    let top = inner.top() - m.top as f32;
    let bar = Rect::from_min_max(pos2(inner.left() - m.left as f32, top), pos2(inner.right() + m.right as f32, top + BAR));
    let p = ui.ctx().layer_painter(ui.layer_id());
    let cr = ui.visuals().window_corner_radius;
    p.rect_filled(bar, egui::CornerRadius { nw: cr.nw, ne: cr.ne, sw: 0, se: 0 }, Color32::from_rgb(228, 240, 252));
    p.line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0_f32, Color32::from_rgb(170, 178, 188)));
    p.text(bar.center(), Align2::CENTER_CENTER, title, FontId::proportional(15.0), TEXT);
    let c = pos2(bar.right() - 20.0, bar.center().y);
    let resp = ui.interact(Rect::from_center_size(c, vec2(28.0, 26.0)), ui.id().with("dialog_close"), Sense::click()).on_hover_text("Schließen");
    // FREO_DEMO_CLOSE_HOVER shows the hover look in demo screenshots
    let hovered = resp.hovered() || std::env::var_os("FREO_DEMO_CLOSE_HOVER").is_some();
    let t = ui.ctx().animate_bool_with_time(resp.id, hovered, 0.12);
    let red = Color32::from_rgb(214, 48, 40);
    let lerp = |a: Color32, b: Color32| Color32::from_rgb((a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8, (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8, (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8);
    let half = 4.5 + 1.8 * t;
    if t > 0.01 {
        let r = Rect::from_center_size(c, vec2(2.0 * half + 9.0, 2.0 * half + 9.0));
        p.rect(r, 3.0, Color32::from_rgba_unmultiplied(214, 48, 40, (28.0 * t) as u8), Stroke::new(1.4_f32, red.gamma_multiply(t)), egui::StrokeKind::Inside);
    }
    let st = Stroke::new(1.3 + 0.5 * t, lerp(Color32::from_rgb(70, 74, 80), red));
    p.line_segment([c + vec2(-half, -half), c + vec2(half, half)], st);
    p.line_segment([c + vec2(-half, half), c + vec2(half, -half)], st);
    let tw = ui.fonts(|f| f.layout_no_wrap(title.to_string(), FontId::proportional(15.0), TEXT).size().x);
    ui.set_min_width(tw + 80.0);
    ui.add_space((BAR - m.top as f32 + 4.0).max(0.0));
    resp.clicked()
}

/// A name for a new folder in `parent` that is not taken yet.
pub fn free_folder_name(parent: &Path) -> String {
    (1..).map(|i| if i == 1 { "Neuer Ordner".to_string() } else { format!("Neuer Ordner {}", i) }).find(|n| !parent.join(n).exists()).unwrap()
}

/// Checks a file or folder name typed by the user.
pub fn valid_name(name: &str) -> Result<&str, &'static str> {
    let n = name.trim();
    if n.is_empty() { return Err("Bitte einen Namen eingeben."); }
    if n.contains('/') || n.contains(':') { return Err("Der Name darf keine / oder : enthalten."); }
    if n.starts_with('.') { return Err("Der Name darf nicht mit einem Punkt beginnen."); }
    Ok(n)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveKind { Part, Assembly, Drawing }

/// What Freo's explorer dialog is choosing a path for.
#[derive(Clone, Debug, PartialEq)]
pub enum BrowsePurpose { WorkDir, SaveAs(SaveKind), Assemble, DrawingModel, BomCsv(String), DrawingExport(String) }

impl BrowsePurpose {
    fn title(&self) -> &'static str {
        match self {
            BrowsePurpose::WorkDir => "Arbeitsverzeichnis auswählen",
            BrowsePurpose::SaveAs(_) => "Speichern unter",
            BrowsePurpose::Assemble => "Komponente zusammenbauen",
            BrowsePurpose::DrawingModel => "Modell für die Zeichnung",
            BrowsePurpose::BomCsv(_) => "Stückliste exportieren",
            BrowsePurpose::DrawingExport(_) => "Zeichnung exportieren",
        }
    }
    pub fn folders_only(&self) -> bool { matches!(self, BrowsePurpose::WorkDir) }
    pub fn saving(&self) -> bool { matches!(self, BrowsePurpose::SaveAs(_) | BrowsePurpose::BomCsv(_) | BrowsePurpose::DrawingExport(_)) }
    pub fn exts(&self) -> Vec<String> {
        match self {
            BrowsePurpose::WorkDir => vec![],
            BrowsePurpose::SaveAs(SaveKind::Part) => vec!["pfprt".into()],
            BrowsePurpose::SaveAs(SaveKind::Assembly) => vec!["pfasm".into()],
            BrowsePurpose::SaveAs(SaveKind::Drawing) => vec!["pfdrw".into()],
            BrowsePurpose::Assemble => vec!["pfprt".into(), "json".into()],
            BrowsePurpose::DrawingModel => vec!["pfprt".into(), "pfasm".into()],
            BrowsePurpose::BomCsv(_) => vec!["csv".into()],
            BrowsePurpose::DrawingExport(k) => vec![k.clone()],
        }
    }
    fn type_label(&self) -> String {
        match self {
            BrowsePurpose::WorkDir => "Ordner".into(),
            BrowsePurpose::SaveAs(SaveKind::Part) | BrowsePurpose::Assemble => "Freo-Teil (.pfprt)".into(),
            BrowsePurpose::SaveAs(SaveKind::Assembly) => "Freo-Baugruppe (.pfasm)".into(),
            BrowsePurpose::SaveAs(SaveKind::Drawing) => "Freo-Zeichnung (.pfdrw)".into(),
            BrowsePurpose::DrawingModel => "Freo-Modelle (.pfprt, .pfasm)".into(),
            BrowsePurpose::BomCsv(_) => "CSV (.csv)".into(),
            BrowsePurpose::DrawingExport(k) => format!("{} (.{})", k.to_uppercase(), k),
        }
    }
    fn ok_label(&self) -> &'static str {
        match self { BrowsePurpose::WorkDir => "OK", BrowsePurpose::Assemble => "Zusammenbauen", BrowsePurpose::DrawingModel => "Öffnen", _ => "Speichern" }
    }
}

/// Freo's own explorer (replaces the macOS panels): folder picker, Speichern unter, Öffnen.
pub struct FileBrowser {
    pub purpose: BrowsePurpose,
    pub dir: PathBuf,
    pub selected: Option<PathBuf>,
    pub name: String,
    pub history: Vec<PathBuf>,
    pub search: String,
    pub keep_at_start: bool,
}

impl FileBrowser {
    /// The path OK would use: the folder (folder mode) or the file with its extension.
    pub fn target(&self) -> Option<PathBuf> {
        if self.purpose.folders_only() { return Some(self.selected.clone().filter(|p| p.is_dir()).unwrap_or_else(|| self.dir.clone())); }
        let n = self.name.trim();
        if n.is_empty() || valid_name(n).is_err() { return None; }
        let exts = self.purpose.exts();
        let has = exts.iter().any(|e| n.to_lowercase().ends_with(&format!(".{}", e)));
        Some(self.dir.join(if has || exts.is_empty() { n.to_string() } else { format!("{}.{}", n, exts[0]) }))
    }
}

#[derive(Default)]
pub struct UiState {
    /// Startseite: no model open, like Creo right after start.
    pub home: bool,
    pub ribbon_min: bool,
    /// a tab clicked while the ribbon is minimised shows it until the next click
    pub peek: bool,
    pub bg: Background,
    pub prev_view: Option<Camera>,
    /// Werkzeuge ▸ Suchen query (dialog open when Some)
    pub search: Option<String>,
    pub ref_viewer: bool,
    pub player: bool,
    pub annot: AnnotPlanes,
    /// Grafiksymbolleiste customization
    pub gfx: super::gfxbar::GfxBar,
    /// Modellraster as applied, and its dialog while open
    pub grid: super::modelgrid::ModelGrid,
    pub grid_dialog: Option<super::modelgrid::GridDialog>,
    /// active Kombinations-Zustand (index into ModelData::combos); None = Standard Alle
    pub combo_active: Option<usize>,
    /// 3D annotation commands, selection and dialogs
    pub annot3: super::annot3d::Annot3Ui,
    /// Kopieren / Einfügen
    pub clip_features: Vec<Feature>,
    /// UDF / Kopiergeometrie / Shrinkwrap dialog
    pub file_task: Option<super::modelcmds::FileTaskDialog>,
    pub regen_manager: bool,
    pub iface_dialog: bool,
    pub open: Option<OpenDialog>,
    pub export: Option<ExportDialog>,
    pub creo_file: Option<CreoFileInfo>,
    pub name_dialog: Option<NameDialog>,
    pub browser: Option<FileBrowser>,
    /// the chosen Arbeitsverzeichnis is used again at the next start
    pub keep_work_dir: bool,
    pub fs_action: Option<FsAction>,
    pub fs: FsCtx,
    /// entry selected in the navigator's folder browser
    pub browser_sel: Option<PathBuf>,
    pub tree_filter: String,
    /// tree columns BENENNUNG / SACHNUMMER hidden
    pub hide_cols: bool,
    /// "Konstruktionselemente" node expanded
    pub design_open: bool,
}

impl UiState {
    /// The ribbon body is hidden (minimised and not peeking).
    pub fn ribbon_min_hidden(&self) -> bool { self.ribbon_min && !self.peek }
}

fn home_dir() -> PathBuf { std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/")) }

impl App {
    // ---------------- Startseite ----------------

    pub fn go_home(&mut self) {
        self.new_part();
        self.messages.clear();
        self.creo.home = true;
        self.tab = Tab::Home;
        self.nav_tab = NavTab::Browser;
        self.messages.push(Message { text: "Willkommen bei Freo.".into(), kind: MsgKind::Info });
    }

    pub fn leave_home(&mut self) {
        if !self.creo.home { return; }
        self.creo.home = false;
        self.nav_tab = NavTab::Tree;
        if self.tab == Tab::Home { self.tab = Tab::Model; }
    }

    pub fn home_ribbon(&mut self, ui: &mut egui::Ui) {
        group(ui, "Daten", |ui| {
            if large(ui, Icon::New, "Neu", true, false).clicked() { self.confirm_new(); }
            if large(ui, Icon::Open, "Öffnen", true, false).clicked() { self.open(); }
            let last = self.recent.first().cloned();
            if large(ui, Icon::LastSession, "Letzte Sitzung\nöffnen", last.is_some(), false).on_hover_text("Die zuletzt geöffnete Datei wieder öffnen").clicked() {
                if let Some(p) = last { self.load(p); }
            }
            if large(ui, Icon::WorkDir, "Arbeitsverzeichnis\nauswählen", true, false).clicked() { self.pick_work_dir(); }
            large(ui, Icon::Erase, "Nicht angezeigte aus\nder Sitzung löschen", false, false);
        });
        group(ui, "Windchill", |ui| { large(ui, Icon::Server, "Serververwaltung", false, false); });
        self.htl_open_group(ui);
        self.htl_display_group(ui);
        let ((), t) = group_drop(ui, "Einstellungen", |ui| {
            large(ui, Icon::ModelDisplay, "Modelldarstellung", false, false);
            large(ui, Icon::SysColors, "Systemfarbeffekt", false, false);
        });
        group_menu(ui, "home_settings", &t, |ui| { if menu_item(ui, Icon::Options, "Maus- und Tastatursteuerung", true) { self.windows.controls = true; } });
        let ((), t) = group_drop(ui, "Dienstprogramme", |ui| { large(ui, Icon::Trail, "Traildatei\nabspielen", false, false); });
        group_menu(ui, "home_utils", &t, |ui| { if menu_item(ui, Icon::Recent, "Meldungsprotokoll", true) { self.windows.messages = true; } });
        group(ui, "Anwendungen", |ui| { large(ui, Icon::Moldbase, "Expert\nMoldbase", false, false); });
    }

    pub fn pick_work_dir(&mut self) {
        let dir = self.work_dir.clone();
        self.browse(BrowsePurpose::WorkDir, dir, "");
    }

    /// Opens Freo's explorer in `dir` for `purpose` (`name` pre-fills the file name).
    pub fn browse(&mut self, purpose: BrowsePurpose, dir: PathBuf, name: &str) {
        let dir = if dir.is_dir() { dir } else { home_dir() };
        let keep_at_start = self.creo.keep_work_dir;
        self.creo.browser = Some(FileBrowser { purpose, dir, selected: None, name: name.to_string(), history: vec![], search: String::new(), keep_at_start });
    }

    /// Carries out what the explorer was opened for.
    pub fn finish_browse(&mut self, purpose: BrowsePurpose, path: PathBuf, keep_at_start: bool) {
        match purpose {
            BrowsePurpose::WorkDir => {
                self.work_dir = path.clone();
                self.browser_dir = path.clone();
                self.creo.keep_work_dir = keep_at_start;
                self.say(format!("Arbeitsverzeichnis: {}{}", path.display(), if keep_at_start { " (auch beim nächsten Start)" } else { "" }), MsgKind::Success);
            }
            BrowsePurpose::SaveAs(SaveKind::Part) => self.save_part_to(path),
            BrowsePurpose::SaveAs(SaveKind::Assembly) => self.asm_save_to(path),
            BrowsePurpose::SaveAs(SaveKind::Drawing) => self.save_drawing_to(path),
            BrowsePurpose::Assemble => { if self.in_asm() && !self.placing() { self.assemble_path(path); } }
            BrowsePurpose::DrawingModel => { if self.confirm_discard() { self.load(path); } }
            BrowsePurpose::BomCsv(csv) => match std::fs::write(&path, csv) {
                Ok(_) => self.say(format!("Stückliste exportiert nach {}.", path.display()), MsgKind::Success),
                Err(e) => self.say(format!("Stücklistenexport fehlgeschlagen: {}", e), MsgKind::Error),
            },
            BrowsePurpose::DrawingExport(k) => self.export_drawing_to(&k, path),
        }
    }

    pub fn file_browser_ui(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.creo.browser.take() else { return };
        let (mut keep, mut x_open) = (true, true);
        let mut finish: Option<PathBuf> = None;
        let mut fs_act: Option<FsAction> = None;
        let fx = self.creo.fs.clone();
        let folders = self.common_folders();
        let exts = d.purpose.exts();
        let max_h = (ctx.screen_rect().height() - 200.0).clamp(240.0, 380.0);
        let title = d.purpose.title();
        egui::Window::new(title).title_bar(false).collapsible(false).resizable(false).fixed_size([900.0, max_h + 150.0]).order(egui::Order::Foreground).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            if dialog_header(ui, title) { x_open = false; }
            // address bar
            ui.horizontal(|ui| {
                if super::ribbon::icon_button(ui, Icon::Undo, "Zurück", !d.history.is_empty(), false, 14.0).clicked() { if let Some(b) = d.history.pop() { d.dir = b; d.selected = None; } }
                if super::ribbon::icon_button(ui, Icon::Up, "Übergeordneter Ordner", d.dir.parent().is_some(), false, 14.0).clicked() {
                    if let Some(p) = d.dir.parent().map(Path::to_path_buf) { d.history.push(d.dir.clone()); d.dir = p; d.selected = None; }
                }
                egui::Frame::NONE.fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, BORDER)).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                    ui.set_width((ui.available_width() - 190.0).max(200.0));
                    egui::ScrollArea::horizontal().id_salt("browse_crumbs").stick_to_right(true).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let comps: Vec<PathBuf> = d.dir.ancestors().map(Path::to_path_buf).collect::<Vec<_>>().into_iter().rev().collect();
                        for c in &comps {
                            let n = c.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Macintosh HD".into());
                            if ui.add(egui::Button::new(RichText::new(n).size(12.5)).frame(false)).clicked() { d.history.push(d.dir.clone()); d.dir = c.clone(); d.selected = None; }
                            ui.label(RichText::new("▸").size(10.0).color(LABEL));
                        }
                    }));
                });
                ui.add(egui::TextEdit::singleline(&mut d.search).hint_text("Suchen...").desired_width(170.0));
            });
            ui.horizontal(|ui| {
                ui.menu_button("Organisieren ▾", |ui| { fs_menu(ui, &d.dir, d.selected.as_deref(), &fx, &mut fs_act); });
                if ui.button("Neuer Ordner").clicked() { fs_act = Some(FsAction::NewFolder(d.dir.clone())); }
            });
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(200.0);
                    ui.set_min_height(max_h);
                    ui.label(RichText::new("▾  Gemeinsame Ordner").strong().size(13.0));
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (icon, label, path) in &folders {
                        let (r, _) = Self::paint_row(ui, 1, *icon, label, if path.is_some() { TEXT } else { DIM_TEXT }, false, path.as_ref() == Some(&d.dir), None);
                        if r.clicked() { if let Some(p) = path { d.history.push(d.dir.clone()); d.dir = p.clone(); d.selected = None; } }
                    }
                });
                ui.add_space(6.0);
                egui::Frame::NONE.fill(Color32::from_rgb(245, 250, 253)).stroke(Stroke::new(1.0_f32, BORDER)).show(ui, |ui| {
                    ui.set_min_size(vec2(ui.available_width(), max_h));
                    let q = d.search.to_lowercase();
                    let mut entries: Vec<(String, bool, PathBuf)> = std::fs::read_dir(&d.dir).map(|rd| rd.filter_map(|e| e.ok()).map(|e| { let p = e.path(); (e.file_name().to_string_lossy().to_string(), p.is_dir(), p) })
                        .filter(|(n, dir, _)| !n.starts_with('.') && (q.is_empty() || n.to_lowercase().contains(&q)) && (*dir || d.purpose.folders_only() || exts.iter().any(|e| n.to_lowercase().ends_with(&format!(".{}", e))))).collect()).unwrap_or_default();
                    entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
                    egui::ScrollArea::vertical().id_salt("browse_list").auto_shrink([false, false]).max_height(max_h).show(ui, |ui| {
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            if entries.is_empty() { ui.label(RichText::new(if d.purpose.folders_only() { "  Keine Unterordner – OK wählt diesen Ordner." } else { "  Keine passenden Dateien in diesem Ordner" }).color(LABEL)); }
                            for (name, is_dir, path) in entries {
                                // in the folder picker files are only shown for orientation
                                let dim = d.purpose.folders_only() && !is_dir;
                                let icon = if is_dir { Icon::FolderItem } else if name.ends_with(".pfasm") { Icon::Assembly } else if name.ends_with(".pfdrw") { Icon::Drawing } else if name.ends_with(".pfprt") { Icon::Part } else { Icon::FileItem };
                                let (r, _) = Self::paint_row(ui, 0, icon, &name, if dim { DIM_TEXT } else { TEXT }, false, d.selected.as_ref() == Some(&path), None);
                                if (r.clicked() || r.secondary_clicked()) && !dim {
                                    d.selected = Some(path.clone());
                                    if !is_dir { d.name = name.clone(); }
                                }
                                if r.double_clicked() {
                                    if is_dir { d.history.push(d.dir.clone()); d.dir = path.clone(); d.selected = None; }
                                    else if !dim && !d.purpose.saving() { finish = Some(path.clone()); }
                                }
                                r.context_menu(|ui| fs_menu(ui, &d.dir, Some(&path), &fx, &mut fs_act));
                            }
                            let h = ui.available_height();
                            let bg = ui.allocate_response(vec2(ui.available_width(), if h.is_finite() { h.clamp(40.0, 2000.0) } else { 120.0 }), Sense::click());
                            super::ui::no_hand(ui.ctx(), bg.id);
                            if bg.clicked() { d.selected = None; }
                            bg.context_menu(|ui| fs_menu(ui, &d.dir, None, &fx, &mut fs_act));
                        });
                    });
                });
            });
            ui.add_space(6.0);
            if d.purpose.folders_only() {
                let t = d.target().unwrap_or_default();
                ui.horizontal(|ui| { ui.label("Ordner:"); ui.label(RichText::new(t.display().to_string()).strong()); });
                ui.checkbox(&mut d.keep_at_start, "Auch beim nächsten Start von Freo verwenden");
            } else {
                ui.horizontal(|ui| {
                    ui.label("Dateiname:");
                    let r = ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(320.0));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { finish = d.target(); }
                    if d.purpose.saving() { if let Some(e) = exts.first() { ui.label(RichText::new(format!(".{}", e)).color(LABEL)); } }
                    ui.add_space(12.0);
                    ui.label("Typ:");
                    ui.label(RichText::new(d.purpose.type_label()).color(LABEL));
                });
            }
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("  Abbrechen  ").min_size(vec2(100.0, 28.0))).clicked() { keep = false; }
                    let target = d.target().filter(|p| if d.purpose.folders_only() { p.is_dir() } else if d.purpose.saving() { true } else { p.is_file() });
                    if ui.add_enabled(target.is_some(), egui::Button::new(RichText::new(d.purpose.ok_label()).strong()).min_size(vec2(110.0, 28.0)).fill(Color32::from_rgb(200, 232, 250))).clicked() { finish = target; }
                });
            });
        });
        if !ctx.wants_keyboard_input() && ctx.input(|i| i.modifiers.command && (i.key_pressed(egui::Key::Backspace) || i.key_pressed(egui::Key::Delete))) {
            if let Some(p) = d.selected.clone().filter(|p| p.exists()) { fs_act = Some(FsAction::Trash(p)); }
        }
        if fs_act.is_some() { self.creo.fs_action = fs_act; }
        if let Some(p) = finish {
            let exists = p.is_file() && d.purpose.saving();
            let ok = !exists || rfd::MessageDialog::new().set_title(title).set_description(format!("{} existiert bereits. Ersetzen?", p.display())).set_buttons(rfd::MessageButtons::YesNo).show() == rfd::MessageDialogResult::Yes;
            if ok {
                let (purpose, keep_start) = (d.purpose.clone(), d.keep_at_start);
                if !d.purpose.folders_only() { self.browser_dir = d.dir.clone(); }
                self.finish_browse(purpose, p, keep_start);
                return;
            }
        }
        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.creo.name_dialog.is_none();
        if keep && x_open && !esc { self.creo.browser = Some(d); }
    }

    /// Folder navigator of the Startseite and the File Open dialog: Creo's "Gemeinsame Ordner".
    fn common_folders(&self) -> Vec<(Icon, String, Option<PathBuf>)> {
        let h = home_dir();
        let docs = h.join("Documents");
        let icloud = h.join("Library/Mobile Documents/com~apple~CloudDocs");
        let mut v: Vec<(Icon, String, Option<PathBuf>)> = vec![
            (Icon::Windows, "In Sitzung".into(), None),
            (Icon::FullScreen, "Desktop".into(), Some(h.join("Desktop"))),
            (Icon::Open, "Downloads".into(), Some(h.join("Downloads")).filter(|p| p.is_dir())),
            (Icon::FolderItem, "Eigene Dokumente".into(), Some(if docs.is_dir() { docs } else { h.clone() })),
        ];
        if icloud.is_dir() { v.push((Icon::Globe, "iCloud Drive".into(), Some(icloud))); }
        v.push((Icon::Globe, "Benutzerordner".into(), Some(h)));
        v.push((Icon::WorkDir, "Arbeitsverzeichnis".into(), Some(self.work_dir.clone())));
        v.push((Icon::WorkDir, "freo-work".into(), Some(super::default_work_dir())));
        // SD cards and USB sticks
        for vol in volumes() { let n = vol.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(); v.push((Icon::Save, n, Some(vol))); }
        v.push((Icon::Recent, "Zuletzt aufgerufen".into(), None));
        v
    }

    pub fn home_navigator(&mut self, ui: &mut egui::Ui) {
        nav_header(ui, self);
        ui.add_space(6.0);
        ui.label(RichText::new("  ▾  Gemeinsame Ordner").strong().size(13.5).color(TEXT));
        ui.add_space(4.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        for (icon, label, path) in self.common_folders() {
            let (r, _) = Self::paint_row(ui, 1, icon, &label, TEXT, false, false, None);
            if let Some(p) = path.clone() {
                let mut act = None;
                let fx = self.creo.fs.clone();
                r.context_menu(|ui| fs_menu(ui, &p, None, &fx, &mut act));
                if act.is_some() { self.creo.fs_action = act; }
            }
            if r.clicked() {
                match (label.as_str(), path) {
                    ("Zuletzt aufgerufen", _) => self.file_menu = true,
                    (_, Some(p)) => { self.browser_dir = p; self.open_in(None); }
                    _ => {}
                }
            }
        }
        ui.add_space(8.0);
        ui.label(RichText::new("  ▸  Netzwerk").strong().size(13.5).color(TEXT));
        ui.add_space(6.0);
        ui.label(RichText::new("  ▸  Ordnerstruktur").strong().size(13.5).color(TEXT));
    }

    // ---------------- extra ribbon tabs ----------------

    pub fn live_sim_ribbon(&mut self, ui: &mut egui::Ui) {
        group_drop(ui, "Studie", |ui| { stack(ui, |ui| { tiny(ui, Icon::AuxApps, "Neue Studie", false, false, true); tiny(ui, Icon::Simulate, "Simulation", false, false, false); }); });
        group(ui, "Setup", |ui| { large(ui, Icon::Contact, "Kontakt", false, false); });
        group(ui, "Randbedingungen", |ui| {
            large(ui, Icon::Fixed, "Fest", false, false);
            large(ui, Icon::Displacement, "Verschiebung", false, false);
            stack(ui, |ui| { tiny(ui, Icon::Contact, "Lager", false, false, false); tiny(ui, Icon::ZoomSel, "Symmetrie", false, false, false); tiny(ui, Icon::Settings, "Fluss", false, false, false); });
        });
        group(ui, "Lasten", |ui| {
            for (ic, n) in [(Icon::Force, "Kraft"), (Icon::Moment, "Moment"), (Icon::Pressure, "Druck"), (Icon::Gravity, "Gravitation"), (Icon::Centrifugal, "Zentrifugal"), (Icon::LinAccel, "Lineare\nBeschleunigung")] {
                large(ui, ic, n, false, false);
            }
        });
        group(ui, "Darstellungsoptionen", |ui| {
            large(ui, Icon::DraftCheck, "Abweichung\nprüfen", false, false);
            split(ui, Icon::Deform, "Verformung\nanimieren", false, false);
            large(ui, Icon::Probe, "Simulationsobjekt-\nAnzeige", false, false);
        });
        group_drop(ui, "Abfrage", |ui| {
            large(ui, Icon::Probe, "Simulations-\nTaster", false, false);
            large(ui, Icon::SimReport, "Simulationsbericht", false, false);
        });
    }

    /// One tile of the Anmerkungsebenen gallery; a click activates that plane. Returns true when clicked.
    fn annot_tile(&mut self, ui: &mut egui::Ui, i: usize) -> bool {
        let pl = &self.creo.annot.list[i];
        let (name, icon) = (pl.name.clone(), pl.icon);
        let sel = self.creo.annot.active == i;
        let (rect, resp) = ui.allocate_exact_size(vec2(TILE_W, super::ribbon::RIB_H), Sense::click());
        let p = ui.painter();
        if sel { p.rect_filled(rect.shrink(1.0), 0.0, Color32::from_rgb(236, 238, 240)); p.rect_stroke(rect.shrink(1.0), 0.0, Stroke::new(1.0_f32, BORDER), egui::StrokeKind::Inside); }
        else if resp.hovered() { p.rect_filled(rect.shrink(1.0), 0.0, HOVER_BG); }
        icons::paint(p, Rect::from_center_size(pos2(rect.center().x, rect.top() + 26.0), vec2(34.0, 34.0)), icon, true);
        // Creo breaks FLAT TO SCREEN after the second word
        let label = if name == "FLAT TO SCREEN" { "FLAT TO\nSCREEN".to_string() } else { name };
        for (k, l) in label.lines().enumerate() { p.text(pos2(rect.center().x, rect.top() + 58.0 + k as f32 * 15.0), Align2::CENTER_CENTER, l, FontId::proportional(12.0), TEXT); }
        let clicked = resp.on_hover_text("Anmerkungsebene festlegen").clicked();
        if clicked { self.set_annot_plane(i); }
        clicked
    }

    /// Makes plane `i` the active annotation plane; with "Modell bei Festlegung neu orientieren" the model turns to face it.
    fn set_annot_plane(&mut self, i: usize) {
        self.creo.annot.active = i;
        let pl = self.creo.annot.list[i].clone();
        if let (true, Some(q)) = (self.creo.annot.reorient, pl.rot) { self.push_view(); self.camera.rot = q; self.fit_pending = true; }
        self.say(format!("Anmerkungsebene {} ist festgelegt.", pl.name), MsgKind::Info);
    }

    /// Anmerkungsebenen-Manager as in Creo: Einstellen / Neu / Editieren / Löschen, the plane list with
    /// its Galeriedarstellung switch, Modellverhalten and the annotation plane grid.
    pub fn annot_manager(&mut self, ctx: &egui::Context) {
        let Some((mut sel, mut edit)) = self.creo.annot.manager.clone() else { return };
        let (mut open, mut close) = (true, false);
        let (mut set, mut new, mut delete) = (false, false, false);
        let n = self.creo.annot.list.len();
        sel = sel.min(n - 1);
        let builtin = self.creo.annot.list[sel].builtin;
        egui::Window::new("Anmerkungsebenen-Manager").id(egui::Id::new("annot_manager")).title_bar(false).collapsible(false).resizable(false).default_pos(pos2(420.0, 120.0)).show(ctx, |ui| {
            if dialog_header(ui, "Anmerkungsebenen-Manager") { open = false; }
            ui.set_width(460.0);
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                let b = |t: &str| egui::Button::new(RichText::new(t).size(14.0)).min_size(vec2(80.0, 30.0));
                if ui.add_enabled(self.creo.annot.active != sel, b("Einstellen")).on_hover_text("Die gewählte Ebene als aktive Anmerkungsebene festlegen").clicked() { set = true; }
                if ui.add(b("Neu")).on_hover_text("Eine Anmerkungsebene aus der aktuellen Ansicht erzeugen").clicked() { new = true; }
                if ui.add_enabled(!builtin && edit.is_none(), b("Editieren")).on_hover_text("Die gewählte Ebene umbenennen").clicked() { edit = Some(self.creo.annot.list[sel].name.clone()); }
                if ui.add_enabled(!builtin, b("Löschen")).clicked() { delete = true; }
            });
            ui.add_space(8.0);
            // plane list: Namen | Galeriedarstellung
            egui::Frame::NONE.fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, BORDER)).inner_margin(2.0).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let w = ui.available_width();
                let split = w * 0.57;
                let (head, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
                let p = ui.painter();
                p.line_segment([pos2(head.left() + split, head.top()), pos2(head.left() + split, head.bottom())], Stroke::new(1.0_f32, BORDER));
                p.line_segment([head.left_bottom(), head.right_bottom()], Stroke::new(1.0_f32, BORDER));
                p.text(pos2(head.left() + 10.0, head.center().y), Align2::LEFT_CENTER, "Namen", FontId::proportional(14.0), TEXT);
                p.text(pos2(head.left() + split + (w - split) / 2.0, head.center().y), Align2::CENTER_CENTER, "Galeriedarstellung", FontId::proportional(14.0), TEXT);
                for i in 0..n {
                    let (r, resp) = ui.allocate_exact_size(vec2(w, 33.0), Sense::click());
                    let name_r = Rect::from_min_max(r.min, pos2(r.left() + split, r.bottom())).shrink2(vec2(2.0, 1.0));
                    let box_r = Rect::from_center_size(pos2(r.left() + split + (w - split) / 2.0, r.center().y), vec2(15.0, 15.0));
                    let box_hit = ui.interact(box_r.expand(4.0), ui.id().with(("annot_gallery_box", i)), Sense::click());
                    let p = ui.painter().clone();
                    p.rect_filled(r.shrink2(vec2(2.0, 1.0)), 0.0, Color32::from_rgb(246, 243, 253));
                    if i == sel { p.rect_filled(name_r, 0.0, Color32::from_rgb(178, 226, 250)); }
                    p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0_f32, Color32::from_rgb(232, 230, 238)));
                    p.line_segment([pos2(r.left() + split, r.top()), pos2(r.left() + split, r.bottom())], Stroke::new(1.0_f32, Color32::from_rgb(232, 230, 238)));
                    let (pname, on) = (self.creo.annot.list[i].name.clone(), self.creo.annot.list[i].gallery);
                    if i == sel && edit.is_some() {
                        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(name_r.shrink2(vec2(4.0, 3.0))));
                        let te = child.add(egui::TextEdit::singleline(edit.as_mut().unwrap()).desired_width(name_r.width() - 10.0));
                        te.request_focus();
                        if te.lost_focus() {
                            if ui.input(|inp| inp.key_pressed(egui::Key::Escape)) { edit = None; }
                            else {
                                let v = edit.take().unwrap().trim().to_uppercase();
                                if !v.is_empty() && !self.creo.annot.list.iter().enumerate().any(|(k, o)| k != i && o.name == v) { self.creo.annot.list[i].name = v; }
                                else if !v.is_empty() { self.say(format!("Eine Anmerkungsebene namens {} gibt es schon.", v), MsgKind::Warning); }
                            }
                        }
                    } else {
                        let font = FontId::proportional(14.5);
                        p.text(pos2(name_r.left() + 6.0, r.center().y), Align2::LEFT_CENTER, &pname, font, if i == self.creo.annot.active { Color32::BLACK } else { TEXT });
                    }
                    check_box(&p, box_r, on);
                    if box_hit.clicked() { self.creo.annot.list[i].gallery = !on; }
                    else if resp.clicked() && !(i == sel && edit.is_some()) { if i != sel { edit = None; } sel = i; }
                    if resp.double_clicked() { set = true; }
                }
                ui.allocate_space(vec2(w, (300.0 - n as f32 * 33.0).max(20.0)));
            });
            ui.add_space(12.0);
            fieldset(ui, "Modellverhalten", |ui| { creo_check(ui, &mut self.creo.annot.reorient, "Modell bei Festlegung neu orientieren"); });
            ui.add_space(10.0);
            fieldset(ui, "Anmerkungsebenen-Raster", |ui| {
                creo_check(ui, &mut self.creo.annot.grid, "Ebenenraster anzeigen");
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Rasterabstand").size(14.0));
                    ui.add_space(14.0);
                    let r = ui.add_enabled(self.creo.annot.grid, egui::TextEdit::singleline(&mut self.creo.annot.grid_text).desired_width(90.0).margin(vec2(8.0, 8.0)));
                    if r.lost_focus() {
                        match self.creo.annot.grid_text.trim().replace(',', ".").parse::<f64>() {
                            Ok(v) if v > 0.0 => self.creo.annot.grid_step = v,
                            _ => self.say("Der Rasterabstand muss eine positive Zahl sein.", MsgKind::Warning),
                        }
                        self.creo.annot.grid_text = format!("{:.6}", self.creo.annot.grid_step);
                    }
                });
            });
            ui.add_space(14.0);
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new("Schließen").size(14.0)).min_size(vec2(100.0, 30.0))).clicked() { close = true; }
            });
        });
        if new {
            let used = |k: usize| self.creo.annot.list.iter().any(|p| p.name == format!("APLANE_{}", k));
            let k = (1..).find(|k| !used(*k)).unwrap();
            self.creo.annot.list.push(AnnotPlane { name: format!("APLANE_{}", k), rot: Some(self.camera.rot), icon: Icon::AnnotPlane, gallery: true, builtin: false });
            sel = self.creo.annot.list.len() - 1;
            edit = Some(self.creo.annot.list[sel].name.clone());
            self.say("Neue Anmerkungsebene aus der aktuellen Ansicht. Namen eingeben, Enter bestätigt.", MsgKind::Prompt);
        }
        if delete && !self.creo.annot.list[sel].builtin {
            let gone = self.creo.annot.list.remove(sel);
            if self.creo.annot.active == sel { self.creo.annot.active = 0; } else if self.creo.annot.active > sel { self.creo.annot.active -= 1; }
            sel = sel.min(self.creo.annot.list.len() - 1);
            edit = None;
            self.say(format!("Anmerkungsebene {} gelöscht.", gone.name), MsgKind::Info);
        }
        if set { self.set_annot_plane(sel); }
        self.creo.annot.manager = if close || !open { None } else { Some((sel, edit)) };
    }
    pub fn annot3d_ribbon(&mut self, ui: &mut egui::Ui) {
        let ((), t) = group_drop(ui, "Kombinations-Zustände", |ui| {
            if super::ribbon::rich_tip(large(ui, Icon::ComboNew, "Neu", !self.in_asm(), false), Icon::ComboNew, "Neu", "Neuen Kombinations-Zustand erzeugen und aktivieren.").clicked() { self.combo_new(); }
            let can_update = !self.in_asm();
            if super::ribbon::rich_tip(large(ui, Icon::ComboUpdate, "Aktualisieren", can_update, false), Icon::ComboUpdate, "Aktualisieren", "Den aktiven Kombinations-Zustand mit der aktuellen Ansicht aktualisieren.").clicked() { self.combo_update(); }
        });
        group_menu(ui, "combo_states", &t, |ui| { if menu_item(ui, Icon::ModelGrid, "Modellraster", true) { self.open_model_grid(); } });
        if std::env::var("FREO_DEMO_POPUP").is_ok_and(|v| v.starts_with("model_grid")) && self.creo.grid_dialog.is_none() {
            self.open_model_grid();
            if let Some(d) = &mut self.creo.grid_dialog { d.draft.shown = true; if std::env::var("FREO_DEMO_POPUP").as_deref() == Ok("model_grid2") { d.tab = 1; } }
        }
        let mut gallery = Rect::NOTHING;
        let mut more_clicked = false;
        // gallery shows the planes with Galeriedarstellung on
        let shown: Vec<usize> = (0..self.creo.annot.list.len()).filter(|i| self.creo.annot.list[*i].gallery).collect();
        let ((), title) = group_drop(ui, "Anmerkungsebenen", |ui| {
            let rows = shown.len().div_ceil(GALLERY_COLS).max(1);
            let row = self.creo.annot.row.min(rows - 1);
            let start = ui.cursor().min;
            for k in row * GALLERY_COLS..(row + 1) * GALLERY_COLS {
                if let Some(&i) = shown.get(k) { self.annot_tile(ui, i); } else { ui.allocate_exact_size(vec2(TILE_W, super::ribbon::RIB_H), Sense::hover()); }
            }
            // Creo's gallery strip: scroll up, scroll down, fold out
            let (strip, _) = ui.allocate_exact_size(vec2(16.0, super::ribbon::RIB_H), Sense::hover());
            ui.painter().rect_stroke(strip, 0.0, Stroke::new(1.0_f32, BORDER), egui::StrokeKind::Inside);
            let h = strip.height() / 3.0;
            let arrow = |k: usize, enabled: bool, tip: &str| {
                let r = Rect::from_min_size(strip.min + vec2(0.0, k as f32 * h), vec2(strip.width(), h)).shrink(1.0);
                let resp = ui.interact(r, ui.id().with(("annot_gallery", k)), if enabled { Sense::click() } else { Sense::hover() }).on_hover_text(tip);
                let p = ui.painter();
                if enabled && resp.hovered() { p.rect_filled(r, 0.0, HOVER_BG); }
                let col = if enabled { Color32::from_rgb(70, 74, 80) } else { Color32::from_rgb(190, 194, 198) };
                let c = r.center();
                let tri = |up: bool, c: egui::Pos2| { let s = if up { -1.0 } else { 1.0 }; vec![c + vec2(-3.5, -2.0 * s), c + vec2(3.5, -2.0 * s), c + vec2(0.0, 2.0 * s)] };
                match k {
                    0 => { p.add(egui::Shape::convex_polygon(tri(true, c), col, Stroke::NONE)); }
                    1 => { p.add(egui::Shape::convex_polygon(tri(false, c), col, Stroke::NONE)); }
                    _ => {
                        p.line_segment([c + vec2(-3.5, -3.5), c + vec2(3.5, -3.5)], Stroke::new(1.2_f32, col));
                        p.add(egui::Shape::convex_polygon(tri(false, c + vec2(0.0, 1.0)), col, Stroke::NONE));
                    }
                }
                resp.clicked()
            };
            if arrow(0, row > 0, "Vorherige Zeile") { self.creo.annot.row = row - 1; }
            if arrow(1, row + 1 < rows, "Nächste Zeile") { self.creo.annot.row = row + 1; }
            if arrow(2, true, "Mehr") { self.creo.annot.gallery_open = !self.creo.annot.gallery_open; more_clicked = true; }
            gallery = Rect::from_min_max(start, strip.right_bottom());
        });
        group_menu(ui, "annot_planes", &title, |ui| {
            if menu_item(ui, Icon::FamilyTable, "Anmerkungsebenen-Manager", true) { self.creo.annot.manager = Some((self.creo.annot.active, None)); }
        });
        if std::env::var("FREO_DEMO_POPUP").as_deref() == Ok("annot_gallery") { self.creo.annot.gallery_open = true; }
        if std::env::var("FREO_DEMO_POPUP").as_deref() == Ok("annot_manager") && self.creo.annot.manager.is_none() { self.creo.annot.manager = Some((1, None)); }
        if self.creo.annot.gallery_open {
            let area = egui::Area::new(egui::Id::new("annot_gallery_fold")).fixed_pos(gallery.left_top()).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).inner_margin(2.0).show(ui, |ui| {
                    menu_spacing(ui);
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.set_max_width(GALLERY_COLS as f32 * TILE_W);
                    let mut picked = false;
                    for row in shown.chunks(GALLERY_COLS) {
                        ui.horizontal(|ui| { for &i in row { picked |= self.annot_tile(ui, i); } });
                    }
                    ui.separator();
                    if menu_row(ui, Some(Icon::FamilyTable), "Anmerkungsebenen-Manager", "", true, false).clicked() {
                        self.creo.annot.manager = Some((self.creo.annot.active, None));
                        picked = true;
                    }
                    if picked { self.creo.annot.gallery_open = false; }
                });
            });
            if area.response.clicked_elsewhere() && !more_clicked { self.creo.annot.gallery_open = false; }
        }
        self.annot3_ribbon_groups(ui);
    }

    pub fn flex_ribbon(&mut self, ui: &mut egui::Ui) {
        group(ui, "Formflächenauswahl", |ui| {
            for n in ["Insel", "Inseln", "Materialschnitt", "Materialschnitte"] { large(ui, Icon::Islands, n, false, false); }
        });
        group(ui, "Suchen", |ui| { stack(ui, |ui| { tiny(ui, Icon::Search, "Geometrieregel", false, false, false); tiny(ui, Icon::Search, "Suchen", false, false, false); }); });
        group(ui, "Transformieren", |ui| {
            split(ui, Icon::FlexMove, "Verschieben", false, false);
            large(ui, Icon::FlexOffset, "Versatz", false, false);
            large(ui, Icon::FlexModify, "Analytische\nGeometrie ändern", false, false);
            stack(ui, |ui| { tiny(ui, Icon::FlexSymmetry, "Spiegeln", false, false, false); tiny(ui, Icon::Pattern, "Ersetzen", false, false, false); tiny(ui, Icon::Csys, "Bearbeiten", false, false, false); });
            stack(ui, |ui| { tiny(ui, Icon::FlexModify, "Neu befestigen", false, false, false); tiny(ui, Icon::FlexModify, "Formfläche ersetzen", false, false, false); });
        });
        group(ui, "Erkennung", |ui| {
            large(ui, Icon::Pattern, "Muster", false, false);
            large(ui, Icon::FlexSymmetry, "Symmetrie", false, false);
            split(ui, Icon::FlexRounds, "Rundungen/\nFasen", false, false);
        });
        group(ui, "KEs editieren", |ui| { stack(ui, |ui| { tiny(ui, Icon::EditDef, "Editieren", false, false, false); tiny(ui, Icon::EditDims, "Bemaßungen", false, false, false); }); });
    }

    pub fn apps_ribbon(&mut self, ui: &mut egui::Ui) {
        group(ui, "Konstruktion", |ui| {
            large(ui, Icon::Weld, "Schweißen", false, false);
            large(ui, Icon::Mold, "Spritzguss/\nGuss", false, false);
            large(ui, Icon::Moldbase, "Expert\nMoldbase", false, false);
            large(ui, Icon::GenDesign, "Generatives\nDesign", false, false);
        });
        group(ui, "Simulation", |ui| {
            large(ui, Icon::Simulate, "Ansys\nSimulation", false, false);
            large(ui, Icon::Simulate, "Simulate", false, false);
            large(ui, Icon::SimResults, "Simulate\nErgebnisse", false, false);
            large(ui, Icon::Flow, "Flow\nAnalysis", false, false);
            large(ui, Icon::MoldFlow, "Spritzgussanalyse", false, false);
        });
        group(ui, "Berechnung", |ui| { split(ui, Icon::Mathcad, "Mathcad", false, false); });
        group(ui, "GD&T", |ui| {
            large(ui, Icon::GdtAdvisor, "GD&T\nAdvisor", false, false);
            large(ui, Icon::EzTol, "EZ Tolerance\nAnalysis", false, false);
        });
        group(ui, "Rendern", |ui| { large(ui, Icon::Render, "Render\nStudio", false, false); });
    }

    fn htl_open_group(&mut self, ui: &mut egui::Ui) {
        group(ui, "Open", |ui| {
            stack(ui, |ui| {
                if small(ui, Icon::Part, "Oeffnen Part", true, false).clicked() { self.open_in(Some(OpenKind::Part)); }
                if small(ui, Icon::Assembly, "Oeffnen Assembly", true, false).clicked() { self.open_in(Some(OpenKind::Assembly)); }
                if small(ui, Icon::Drawing, "Oeffnen Drawing", true, false).clicked() { self.open_in(Some(OpenKind::Drawing)); }
            });
        });
    }

    fn htl_display_group(&mut self, ui: &mut egui::Ui) {
        group(ui, "Display", |ui| {
            stack(ui, |ui| {
                for (ic, n, b) in [(Icon::BgWhite, "Hintergrund Weiss", Background::White), (Icon::BgStd, "Hintergrund Standard", Background::Standard), (Icon::BgDark, "Hintergrund Dunkel", Background::Dark)] {
                    if small(ui, ic, n, true, self.creo.bg == b).clicked() { self.creo.bg = b; }
                }
            });
        });
    }

    /// The school's custom tab (HTL template add-ins).
    pub fn htl_ribbon(&mut self, ui: &mut egui::Ui) {
        self.htl_open_group(ui);
        group(ui, "Add ons", |ui| {
            stack(ui, |ui| {
                if small(ui, Icon::Parameters, "Modelleigenschaften", true, self.windows.info).clicked() { self.windows.info = !self.windows.info; }
                small(ui, Icon::Pdf3d, "Exportieren eines 3D PDF", false, false);
                if small(ui, Icon::MassCalc, "Massenberechnung", true, self.windows.mass).clicked() { self.windows.mass = !self.windows.mass; }
            });
            stack(ui, |ui| {
                small(ui, Icon::Drawing, "Erstellt eine Ansicht \"3D-Drawing\"", false, false);
                small(ui, Icon::ImportParams, "Import admCreoHTL-Parameter", false, false);
                if small(ui, Icon::Export, "Exportieren (STL, OBJ, STEP, 3MF ...)", true, self.creo.export.is_some()).clicked() { self.export_dialog(None); }
            });
        });
        self.htl_display_group(ui);
    }

    // ---------------- Datei öffnen ----------------

    pub fn open_in(&mut self, kind: Option<OpenKind>) {
        if self.busy() { self.say("Vor dem Öffnen eines anderen Modells das aktive KE abschließen.", MsgKind::Warning); return; }
        let dir = self.browser_dir.clone();
        self.creo.open = Some(OpenDialog { dir, selected: None, name: String::new(), kind: kind.unwrap_or(OpenKind::All), search: String::new(), history: vec![], insert: false, found: None, pending: None });
    }

    pub fn open_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.creo.open.take() else { return };
        let mut keep = true;
        let mut load: Option<PathBuf> = None;
        let mut fs_act: Option<FsAction> = None;
        let fx = self.creo.fs.clone();
        let folders = self.common_folders();
        let max_h = (ctx.screen_rect().height() - 160.0).clamp(300.0, 420.0);
        let mut x_open = true;
        let dir_before = d.dir.clone();
        let (mut start_models, mut start_name, mut leave_search, mut goto): (bool, Option<String>, bool, Option<PathBuf>) = (false, None, false, None);
        // a background Spotlight search finished?
        let done = d.pending.as_ref().map(|(_, rx)| rx.try_recv());
        match done {
            Some(Ok(v)) => { let t = d.pending.take().map(|x| x.0).unwrap_or_default(); d.found = Some((t, v)); }
            Some(Err(std::sync::mpsc::TryRecvError::Empty)) => ctx.request_repaint_after(std::time::Duration::from_millis(150)),
            Some(Err(_)) => d.pending = None,
            None => {}
        }
        egui::Window::new(if d.insert { "Importieren" } else { "Datei öffnen" }).title_bar(false).collapsible(false).resizable(false).fixed_size([920.0, max_h + 130.0]).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            if dialog_header(ui, if d.insert { "Importieren" } else { "Datei öffnen" }) { x_open = false; }
            // address bar
            ui.horizontal(|ui| {
                let back = d.history.last().cloned();
                if super::ribbon::icon_button(ui, Icon::Undo, "Zurück", back.is_some(), false, 14.0).clicked() { if let Some(b) = d.history.pop() { d.dir = b; } }
                if super::ribbon::icon_button(ui, Icon::Up, "Übergeordneter Ordner", d.dir.parent().is_some(), false, 14.0).clicked() {
                    if let Some(p) = d.dir.parent().map(Path::to_path_buf) { d.history.push(d.dir.clone()); d.dir = p; }
                }
                egui::Frame::NONE.fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, BORDER)).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                    ui.set_width((ui.available_width() - 190.0).max(200.0));
                    egui::ScrollArea::horizontal().id_salt("crumbs").stick_to_right(true).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let comps: Vec<PathBuf> = d.dir.ancestors().map(Path::to_path_buf).collect::<Vec<_>>().into_iter().rev().collect();
                        for c in comps.iter().skip(comps.len().saturating_sub(5)) {
                            let n = c.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Macintosh HD".into());
                            if ui.add(egui::Button::new(RichText::new(n).size(12.5)).frame(false)).clicked() { d.history.push(d.dir.clone()); d.dir = c.clone(); }
                            ui.label(RichText::new("▸").size(10.0).color(LABEL));
                        }
                    }));
                });
                let r = ui.add(egui::TextEdit::singleline(&mut d.search).hint_text("Suchen...").desired_width(170.0));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !d.search.trim().is_empty() { start_name = Some(d.search.trim().to_string()); }
            });
            ui.horizontal(|ui| {
                ui.menu_button("Organisieren ▾", |ui| { fs_menu(ui, &d.dir, d.selected.as_deref(), &fx, &mut fs_act); });
                if ui.add(egui::Button::new(RichText::new("🔍  Modelle auf diesem Mac finden").color(Color32::from_rgb(20, 90, 190)))).on_hover_text("Findet Creo- (.prt, .asm, .drw), STEP- und Freo-Dateien überall auf dem Mac, auf SD-Karten und USB-Sticks").clicked() { start_models = true; }
                let q = d.search.trim().to_string();
                if ui.add_enabled(!q.is_empty(), egui::Button::new(format!("„{}“ auf diesem Mac suchen", if q.is_empty() { "…" } else { q.as_str() }))).on_hover_text("Den Suchbegriff oben rechts überall suchen (Enter im Suchfeld)").clicked() { start_name = Some(q.clone()); }
                for n in ["Ansichten", "Werkzeuge"] { ui.add_enabled(false, egui::Button::new(format!("{} ▾", n))); }
                ui.add_enabled(false, egui::Button::new("Vorschau"));
            });
            ui.separator();
            let list_h = max_h - 40.0;
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(200.0);
                    ui.set_min_height(list_h);
                    ui.label(RichText::new("▾  Gemeinsame Ordner").strong().size(13.0));
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (icon, label, path) in &folders {
                        let active = path.as_ref() == Some(&d.dir);
                        let (r, _) = Self::paint_row(ui, 1, *icon, label, if path.is_some() { TEXT } else { DIM_TEXT }, false, active && d.found.is_none(), None);
                        if r.clicked() { if let Some(p) = path { d.history.push(d.dir.clone()); d.dir = p.clone(); d.found = None; } }
                    }
                    ui.add_space(4.0);
                    let (r, _) = Self::paint_row(ui, 1, Icon::Search, "Modelle auf diesem Mac", Color32::from_rgb(30, 100, 200), false, d.found.as_ref().map(|f| f.0.starts_with("Creo")).unwrap_or(false), None);
                    if r.on_hover_text("Alle Creo-, STEP- und Freo-Dateien auf dem Mac und angeschlossenen Laufwerken finden").clicked() { start_models = true; }
                    ui.add_space(8.0);
                    ui.label(RichText::new("▸  Netzwerk").strong().size(13.0));
                    ui.add_space(4.0);
                    ui.label(RichText::new("▸  Ordnerstruktur").strong().size(13.0));
                });
                ui.add_space(6.0);
                egui::Frame::NONE.fill(Color32::from_rgb(245, 250, 253)).stroke(Stroke::new(1.0_f32, BORDER)).show(ui, |ui| {
                    ui.set_min_size(vec2(ui.available_width(), list_h));
                    if d.found.is_some() || d.pending.is_some() {
                        egui::ScrollArea::vertical().id_salt("open_found").auto_shrink([false, false]).max_height(list_h).show(ui, |ui| {
                            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                ui.horizontal(|ui| {
                                    let (t, n) = match (&d.found, &d.pending) { (_, Some((t, _))) => (t.clone(), None), (Some((t, f)), None) => (t.clone(), Some(f.len())), _ => (String::new(), None) };
                                    ui.label(RichText::new(format!("  {}", t)).strong());
                                    match n { Some(n) => { ui.label(RichText::new(format!("– {} gefunden", n)).color(LABEL)); } None => { ui.spinner(); ui.label(RichText::new("Suche läuft …").color(LABEL)); } }
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { if ui.small_button("× Suche beenden").clicked() { leave_search = true; } });
                                });
                                ui.separator();
                                ui.spacing_mut().item_spacing.y = 0.0;
                                if let Some((_, files)) = &d.found {
                                    if files.is_empty() { ui.label(RichText::new("  Nichts gefunden. Ist die SD-Karte oder der USB-Stick angeschlossen?").color(LABEL)); }
                                    for p in files.clone() {
                                        let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                                        let parent = p.parent().map(Path::to_path_buf).unwrap_or_default();
                                        let ln = name.to_lowercase();
                                        let icon = if is_creo_kind(&ln, "asm") || ln.ends_with(".pfasm") { Icon::Assembly } else if is_creo_kind(&ln, "drw") || ln.ends_with(".pfdrw") { Icon::Drawing } else if is_neutral_file(&ln) { Icon::GetData } else { Icon::Part };
                                        let (r, _) = Self::paint_row(ui, 0, icon, &name, TEXT, false, d.selected.as_ref() == Some(&p), None);
                                        // where it lives, dimmed after the name
                                        let nw = ui.fonts(|f| f.layout_no_wrap(name.clone(), FontId::proportional(12.5), TEXT).size().x);
                                        let home = home_dir();
                                        let shown = parent.strip_prefix(&home).map(|r| format!("~/{}", r.display())).unwrap_or_else(|_| parent.display().to_string());
                                        ui.painter().text(pos2(r.rect.left() + 46.0 + nw, r.rect.center().y), Align2::LEFT_CENTER, shown, FontId::proportional(11.5), LABEL);
                                        if r.clicked() || r.secondary_clicked() { d.selected = Some(p.clone()); d.name = name.clone(); }
                                        if r.double_clicked() { load = Some(p.clone()); }
                                        let r = r.on_hover_text(p.display().to_string());
                                        r.context_menu(|ui| {
                                            menu_spacing(ui);
                                            if menu_entry(ui, Icon::FolderItem, "Enthaltenden Ordner öffnen", true, false).clicked() { goto = Some(parent.clone()); ui.close_menu(); }
                                            ui.separator();
                                            fs_menu(ui, &parent, Some(&p), &fx, &mut fs_act);
                                        });
                                    }
                                }
                            });
                        });
                    } else {
                    let q = d.search.to_lowercase();
                    let mut entries: Vec<(String, bool, PathBuf)> = std::fs::read_dir(&d.dir).map(|rd| rd.filter_map(|e| e.ok()).map(|e| {
                        let p = e.path();
                        (e.file_name().to_string_lossy().to_string(), p.is_dir(), p)
                    }).filter(|(n, dir, _)| !n.starts_with('.') && (*dir || d.kind.accepts(n)) && (q.is_empty() || n.to_lowercase().contains(&q))).collect()).unwrap_or_default();
                    entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
                    egui::ScrollArea::vertical().id_salt("open_list").auto_shrink([false, false]).max_height(list_h).show(ui, |ui| {
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if entries.is_empty() { ui.label(RichText::new("  Keine passenden Dateien in diesem Ordner").color(LABEL)); }
                        for (name, is_dir, path) in entries {
                            let ln = name.to_lowercase();
                            let icon = if is_dir { Icon::FolderItem } else if ln.ends_with(".pfasm") || (is_creo_file(&ln) && ln.contains(".asm")) { Icon::Assembly } else if ln.ends_with(".pfdrw") || (is_creo_file(&ln) && ln.contains(".drw")) { Icon::Drawing } else if is_neutral_file(&ln) { Icon::GetData } else { Icon::Part };
                            let sel = d.selected.as_ref() == Some(&path);
                            let (r, _) = Self::paint_row(ui, 0, icon, &name, TEXT, false, sel, None);
                            if r.clicked() || r.secondary_clicked() { d.selected = Some(path.clone()); if !is_dir { d.name = name.clone(); } }
                            if r.double_clicked() { if is_dir { d.history.push(d.dir.clone()); d.dir = path.clone(); d.selected = None; } else { load = Some(path.clone()); } }
                            r.context_menu(|ui| fs_menu(ui, &d.dir, Some(&path), &fx, &mut fs_act));
                        }
                        // right-click on the empty part of the list
                        let h = ui.available_height();
                        let rest = vec2(ui.available_width(), if h.is_finite() { h.clamp(40.0, 2000.0) } else { 120.0 });
                        let bg = ui.allocate_response(rest, Sense::click());
                        super::ui::no_hand(ui.ctx(), bg.id);
                        bg.context_menu(|ui| fs_menu(ui, &d.dir, None, &fx, &mut fs_act));
                        });
                    });
                    }
                });
            });
            ui.add_space(4.0);
            if let Some(p) = &d.selected {
                let modified = std::fs::metadata(p).and_then(|m| m.modified()).ok().map(|t| {
                    let secs = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
                    let days = secs.div_euclid(86_400);
                    let (y, m, dd) = civil_from_days(days);
                    let rem = secs.rem_euclid(86_400);
                    format!("{:02}.{:02}.{} {:02}:{:02}", dd, m, y, rem / 3600, rem % 3600 / 60)
                }).unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Dateiname:").strong()); ui.label(&d.name);
                    ui.label(RichText::new("Änderungsdatum:").strong()); ui.label(modified);
                });
            }
            ui.horizontal(|ui| {
                ui.label("Dateiname:");
                ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(280.0));
                ui.add_space(12.0);
                ui.label("Typ");
                egui::ComboBox::from_id_salt("open_kind").width(250.0).selected_text(d.kind.label()).show_ui(ui, |ui| {
                    for k in OpenKind::ALL { ui.selectable_value(&mut d.kind, k, k.label()); }
                });
            });
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("   Abbrechen   ")).clicked() { keep = false; }
                    let target = if d.found.is_some() { d.selected.clone() } else if d.name.is_empty() { None } else { Some(d.dir.join(&d.name)) };
                    let target = target.filter(|p| p.is_file());
                    if ui.add_enabled(target.is_some(), egui::Button::new(RichText::new(if d.insert { "    Importieren    " } else { "      Öffnen      " }).strong()).fill(Color32::from_rgb(200, 232, 250))).clicked() { load = target; }
                });
            });
        });
        if !ctx.wants_keyboard_input() && ctx.input(|i| i.modifiers.command && (i.key_pressed(egui::Key::Backspace) || i.key_pressed(egui::Key::Delete))) {
            if let Some(p) = d.selected.clone().filter(|p| p.exists()) { fs_act = Some(FsAction::Trash(p)); }
        }
        // any navigation leaves the search results
        if leave_search || d.dir != dir_before { d.found = None; d.pending = None; }
        if let Some(g) = goto { d.history.push(d.dir.clone()); d.dir = g; d.found = None; d.pending = None; }
        let kind = if d.insert { OpenKind::Import } else { OpenKind::All };
        if start_models { d.found = None; d.selected = None; d.pending = Some(("Creo-, STEP- und Freo-Dateien auf diesem Mac".into(), spotlight(SpotQuery::Models, kind))); }
        if let Some(t) = start_name { d.found = None; d.selected = None; d.pending = Some((format!("„{}“ auf diesem Mac", t), spotlight(SpotQuery::Name(t), kind))); }
        if fs_act.is_some() { self.creo.fs_action = fs_act; }
        if let Some(p) = load {
            self.browser_dir = d.dir.clone();
            if d.insert { self.import_into_part(&p); }
            else if self.confirm_discard() { self.load(p); }
            return;
        }
        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.creo.name_dialog.is_none();
        if keep && x_open && !esc { self.creo.open = Some(d); }
    }

    // ---------------- Ordner anlegen / umbenennen ----------------

    /// Points every remembered path at `new` after `old` was moved or renamed.
    fn repath(&mut self, old: &Path, new: &Path) {
        let fix = |p: &mut PathBuf| { if let Ok(rest) = p.strip_prefix(old) { *p = new.join(rest); } };
        if let Some(p) = &mut self.path { fix(p); }
        if let Some(d) = &mut self.drw { if let Some(p) = &mut d.path { fix(p); } }
        fix(&mut self.browser_dir);
        fix(&mut self.work_dir);
        for r in &mut self.recent { fix(r); }
        if let Some(d) = &mut self.creo.open { fix(&mut d.dir); if let Some(s) = &mut d.selected { fix(s); } }
        if let Some(d) = &mut self.creo.export { fix(&mut d.dir); }
        if let Some(d) = &mut self.creo.browser { fix(&mut d.dir); if let Some(s) = &mut d.selected { fix(s); } }
        if let Some((c, _)) = &mut self.creo.fs.clip { fix(c); }
    }

    /// Paths of folders shown somewhere must stay valid when they are deleted.
    fn leave_deleted(&mut self, gone: &Path) {
        let up = |p: &mut PathBuf| { if p.starts_with(gone) { *p = gone.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("/")); } };
        up(&mut self.browser_dir);
        if let Some(d) = &mut self.creo.open { up(&mut d.dir); if d.selected.as_deref().map(|s| s.starts_with(gone)).unwrap_or(false) { d.selected = None; d.name.clear(); } }
        if let Some(d) = &mut self.creo.export { up(&mut d.dir); }
        if let Some(d) = &mut self.creo.browser { up(&mut d.dir); if d.selected.as_deref().map(|s| s.starts_with(gone)).unwrap_or(false) { d.selected = None; } }
        if self.creo.browser_sel.as_deref().map(|s| s.starts_with(gone)).unwrap_or(false) { self.creo.browser_sel = None; }
        self.recent.retain(|r| !r.starts_with(gone));
        if self.creo.fs.clip.as_ref().map(|c| c.0.starts_with(gone)).unwrap_or(false) { self.creo.fs.clip = None; }
    }

    /// Runs one file operation from the right-click menu or a shortcut.
    pub fn fs_do(&mut self, a: FsAction, ctx: Option<&egui::Context>) {
        match a {
            FsAction::NewFolder(parent) => { let name = free_folder_name(&parent); self.creo.name_dialog = Some(NameDialog { parent, name, rename: None, error: None, focus: true }); }
            FsAction::Rename(p) => {
                let name = short(&p);
                let parent = p.parent().map(Path::to_path_buf).unwrap_or_default();
                self.creo.name_dialog = Some(NameDialog { parent, name, rename: Some(p), error: None, focus: true });
            }
            FsAction::Reveal(p) => { let _ = if p.is_dir() { std::process::Command::new("open").arg(&p).spawn() } else { std::process::Command::new("open").arg("-R").arg(&p).spawn() }; }
            FsAction::Enter(p) => {
                if p.is_dir() {
                    if let Some(d) = &mut self.creo.browser { d.history.push(d.dir.clone()); d.dir = p; d.selected = None; }
                    else if let Some(d) = &mut self.creo.open { d.history.push(d.dir.clone()); d.dir = p; }
                    else if let Some(d) = &mut self.creo.export { d.history.push(d.dir.clone()); d.dir = p; }
                    else { self.browser_dir = p; }
                } else {
                    let n = short(&p).to_lowercase();
                    let ours = n.ends_with(".pfprt") || n.ends_with(".pfasm") || n.ends_with(".pfdrw") || is_neutral_file(&n) || is_creo_file(&n);
                    if !ours { let _ = std::process::Command::new("open").arg(&p).spawn(); }
                    else if is_creo_file(&n) || self.confirm_discard() { self.creo.open = None; self.load(p); }
                }
            }
            FsAction::Copy(p) => { self.say(format!("„{}“ kopiert – im Zielordner Einfügen wählen.", short(&p)), MsgKind::Info); self.creo.fs.clip = Some((p, false)); }
            FsAction::Cut(p) => { self.say(format!("„{}“ ausgeschnitten – im Zielordner Einfügen wählen.", short(&p)), MsgKind::Info); self.creo.fs.clip = Some((p, true)); }
            FsAction::Paste(dir) => {
                let Some((src, cut)) = self.creo.fs.clip.clone() else { return };
                if dir.starts_with(&src) { self.say("Ein Ordner kann nicht in sich selbst eingefügt werden.", MsgKind::Warning); return; }
                if cut && src.parent() == Some(dir.as_path()) { self.creo.fs.clip = None; return; }
                let dst = copy_name(&dir, &src);
                let res = if cut { std::fs::rename(&src, &dst).or_else(|_| copy_recursive(&src, &dst).and_then(|_| if src.is_dir() { std::fs::remove_dir_all(&src) } else { std::fs::remove_file(&src) })) } else { copy_recursive(&src, &dst) };
                match res {
                    Ok(_) => {
                        if cut { self.repath(&src, &dst); self.creo.fs.clip = None; }
                        self.say(format!("„{}“ {} nach {}.", short(&dst), if cut { "verschoben" } else { "kopiert" }, dir.display()), MsgKind::Success);
                    }
                    Err(e) => self.say(format!("Einfügen fehlgeschlagen: {}", e), MsgKind::Error),
                }
            }
            FsAction::Duplicate(p) => {
                let Some(dir) = p.parent() else { return };
                let dst = copy_name(dir, &p);
                match copy_recursive(&p, &dst) { Ok(_) => self.say(format!("„{}“ erstellt.", short(&dst)), MsgKind::Success), Err(e) => self.say(format!("Duplizieren fehlgeschlagen: {}", e), MsgKind::Error) }
            }
            FsAction::Trash(p) => {
                let open_model = self.path.as_deref().map(|m| m.starts_with(&p)).unwrap_or(false);
                match crate::macos_open::trash(&p) {
                    Ok(to) => {
                        self.leave_deleted(&p);
                        self.say(format!("„{}“ in den Papierkorb gelegt{}. Rechtsklick ▸ Zurücklegen holt es zurück.", short(&p), if open_model { " – das offene Modell bleibt in der Sitzung und kann neu gespeichert werden" } else { "" }), MsgKind::Success);
                        if open_model { self.path = None; self.dirty = true; }
                        self.creo.fs.trashed = Some((p, to));
                    }
                    Err(e) => self.say(format!("Löschen fehlgeschlagen: {}", e), MsgKind::Error),
                }
            }
            FsAction::PutBack => {
                let Some((orig, at)) = self.creo.fs.trashed.take() else { return };
                if orig.exists() { self.say(format!("„{}“ gibt es dort schon wieder.", short(&orig)), MsgKind::Warning); return; }
                if let Some(parent) = orig.parent() { let _ = std::fs::create_dir_all(parent); }
                match std::fs::rename(&at, &orig) { Ok(_) => self.say(format!("„{}“ zurückgelegt.", short(&orig)), MsgKind::Success), Err(e) => self.say(format!("Zurücklegen fehlgeschlagen: {}", e), MsgKind::Error) }
            }
            FsAction::CopyPath(p) => { if let Some(c) = ctx { c.copy_text(p.display().to_string()); } self.say(format!("Pfad kopiert: {}", p.display()), MsgKind::Info); }
        }
    }

    fn run_fs_action(&mut self, ctx: &egui::Context) {
        if let Some(a) = self.creo.fs_action.take() { self.fs_do(a, Some(ctx)); }
    }

    /// Creates the folder (or renames the entry); returns the new path.
    pub fn apply_name_dialog(&mut self, nd: &NameDialog) -> Result<PathBuf, String> {
        let n = valid_name(&nd.name).map_err(str::to_string)?;
        let target = nd.parent.join(n);
        if let Some(old) = &nd.rename { if *old == target { return Ok(target); } }
        if target.exists() { return Err(format!("„{}“ gibt es in diesem Ordner schon.", n)); }
        match &nd.rename {
            None => std::fs::create_dir(&target).map_err(|e| format!("Ordner konnte nicht erstellt werden: {}", e))?,
            Some(old) => {
                std::fs::rename(old, &target).map_err(|e| format!("Umbenennen fehlgeschlagen: {}", e))?;
                // keep paths that pointed at the renamed entry (or into a renamed folder)
                let old = old.clone();
                self.repath(&old, &target);
            }
        }
        Ok(target)
    }

    pub fn name_dialog_ui(&mut self, ctx: &egui::Context) {
        self.run_fs_action(ctx);
        let Some(mut nd) = self.creo.name_dialog.take() else { return };
        let (mut keep, mut ok) = (true, false);
        let title = if nd.rename.is_some() { "Umbenennen" } else { "Neuer Ordner" };
        let mut x_open = true;
        egui::Window::new(title).title_bar(false).collapsible(false).resizable(false).order(egui::Order::Foreground).anchor(Align2::CENTER_CENTER, vec2(0.0, -80.0)).show(ctx, |ui| {
            if dialog_header(ui, title) { x_open = false; }
            ui.set_width(380.0);
            ui.label(RichText::new(format!("In: {}", nd.parent.display())).size(11.5).color(LABEL));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Name:");
                let out = egui::TextEdit::singleline(&mut nd.name).desired_width(300.0).show(ui);
                let r = out.response;
                if nd.focus {
                    // whole name selected, so typing replaces it (for a rename: the part before the extension)
                    r.request_focus();
                    let end = match (&nd.rename, nd.name.rfind('.')) { (Some(p), Some(i)) if p.is_file() && i > 0 => nd.name[..i].chars().count(), _ => nd.name.chars().count() };
                    let mut st = out.state;
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(end))));
                    st.store(ui.ctx(), r.id);
                    nd.focus = false;
                }
                if r.changed() { nd.error = None; }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { ok = true; }
            });
            if let Some(e) = &nd.error { ui.label(RichText::new(format!("⚠ {}", e)).color(Color32::from_rgb(200, 36, 28)).size(12.0)); }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("  Abbrechen  ").min_size(vec2(90.0, 26.0))).clicked() { keep = false; }
                    if ui.add(egui::Button::new("OK").min_size(vec2(90.0, 26.0))).clicked() { ok = true; }
                });
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) { keep = false; }
        if !x_open { keep = false; ok = false; }
        if ok {
            match self.apply_name_dialog(&nd) {
                Ok(p) => {
                    let n = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    self.say(if nd.rename.is_some() { format!("Umbenannt in „{}“.", n) } else { format!("Ordner „{}“ erstellt.", n) }, MsgKind::Success);
                    keep = false;
                }
                Err(e) => nd.error = Some(e),
            }
        }
        if keep { self.creo.name_dialog = Some(nd); }
    }

    // ---------------- Importieren / Creo-Dateien ----------------

    /// Daten abrufen ▸ Importieren: STEP/STL/OBJ geometry into the current part.
    pub fn import_dialog(&mut self) {
        if self.busy() { self.say("Zuerst das aktive KE abschließen.", MsgKind::Warning); return; }
        self.open_in(Some(OpenKind::Import));
        if let Some(d) = &mut self.creo.open { d.insert = true; }
    }

    /// Reads a neutral file into an import feature definition.
    pub fn read_neutral(path: &Path) -> Result<(ImportDef, String, String), String> {
        let lower = path.to_string_lossy().to_lowercase();
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let (res, fmt) = if lower.ends_with(".stl") { (crate::model::step_import::import_stl(&bytes)?, "STL") }
            else if lower.ends_with(".obj") { (crate::model::step_import::import_obj(&String::from_utf8_lossy(&bytes))?, "OBJ") }
            else { (crate::model::step_import::import(&String::from_utf8_lossy(&bytes))?, "STEP") };
        let faces = res.faces.iter().map(|f| ImportFace {
            v: f.verts.iter().map(|(p, n)| [p.x as f32, p.y as f32, p.z as f32, n.x as f32, n.y as f32, n.z as f32]).collect(),
            t: f.tris.clone(),
        }).collect();
        let tris: usize = res.faces.iter().map(|f| f.tris.len()).sum();
        let mut note = format!("{} Flächen, {} Dreiecke, Einheit {}", res.faces.len(), tris, res.unit);
        if res.solids > 1 { note.push_str(&format!(", {} Körper", res.solids)); }
        if res.skipped > 0 { note.push_str(&format!(", {} Fläche(n) übersprungen", res.skipped)); }
        let source = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        Ok((ImportDef { source, format: fmt.into(), faces }, res.name, note))
    }

    /// Opening a STEP/STL/OBJ file: a new part holding the geometry as an import feature.
    pub fn open_neutral(&mut self, path: &Path) {
        match Self::read_neutral(path) {
            Ok((def, _, note)) => {
                let stem = path.file_stem().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_else(|| "IMPORT".into());
                let name: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
                self.new_part_named(&name);
                let id = self.data.next_id;
                let fname = self.data.next_name(&FeatureKind::Import(def.clone()));
                self.commit("", |d| { d.features.push(Feature { id, name: fname, suppressed: false, hidden: false, kind: FeatureKind::Import(def) }); d.next_id += 1; });
                self.undo.clear();
                self.dirty = false;
                self.fit_pending = true;
                self.say(format!("{} importiert ({}).", path.display(), note), MsgKind::Success);
                self.add_recent(&path.to_path_buf());
                if let Some(dir) = path.parent() { self.browser_dir = dir.to_path_buf(); }
            }
            Err(e) => self.say(format!("Import fehlgeschlagen: {}", e), MsgKind::Error),
        }
    }

    pub fn import_into_part(&mut self, path: &Path) {
        if self.in_asm() { self.say("In Baugruppen über Zusammenbauen einbauen; Import geht in Teile.", MsgKind::Warning); return; }
        if is_creo_file(&path.to_string_lossy()) { self.show_creo_file(path); return; }
        match Self::read_neutral(path) {
            Ok((def, _, note)) => {
                let id = self.data.next_id;
                let fname = self.data.next_name(&FeatureKind::Import(def.clone()));
                self.commit(&format!("{} importiert ({}).", path.display(), note), |d| {
                    let f = Feature { id, name: fname, suppressed: false, hidden: false, kind: FeatureKind::Import(def) };
                    match d.insert_index { Some(i) => { d.features.insert(i, f); d.insert_index = Some(i + 1); } None => d.features.push(f) }
                    d.next_id += 1;
                });
                self.fit_pending = true;
            }
            Err(e) => self.say(format!("Import fehlgeschlagen: {}", e), MsgKind::Error),
        }
    }

    /// A native Creo file: show what can be read and how to bring the geometry over.
    pub fn show_creo_file(&mut self, path: &Path) {
        let (kind, model, version, machine, img) = read_creo_header(path).unwrap_or_default();
        let model = if model.is_empty() { path.file_stem().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_default() } else { model };
        // a STEP export lying next to it can be opened directly
        let stem = model.to_lowercase();
        let step = path.parent().and_then(|d| std::fs::read_dir(d).ok()).and_then(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
            let n = p.file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            (n.ends_with(".stp") || n.ends_with(".step")) && n.starts_with(&stem)
        }));
        self.creo.creo_file = Some(CreoFileInfo { path: path.to_path_buf(), kind, model, version, machine, thumb: None, step });
        if let Some(img) = img { self.creo_thumb_pending = Some(img); }
        self.say(format!("{} ist eine Creo-Datei im geschlossenen PTC-Format.", path.display()), MsgKind::Warning);
    }

    pub fn creo_file_ui(&mut self, ctx: &egui::Context) {
        if let Some(img) = self.creo_thumb_pending.take() {
            if let Some(info) = &mut self.creo.creo_file { info.thumb = Some(ctx.load_texture("creo_thumb", img, egui::TextureOptions::LINEAR)); }
        }
        let Some(info) = self.creo.creo_file.take() else { return };
        let mut keep = true;
        let mut open_step: Option<PathBuf> = None;
        let mut pick_step = false;
        let mut x_open = true;
        egui::Window::new("Creo-Datei").title_bar(false).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, -30.0)).default_width(560.0).show(ctx, |ui| {
            if dialog_header(ui, "Creo-Datei") { x_open = false; }
            ui.horizontal_top(|ui| {
                if let Some(t) = &info.thumb { ui.add(egui::Image::new(t).fit_to_exact_size(vec2(180.0, 180.0))); }
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("{}.{}", info.model, match info.kind.as_str() { "ASSEMBLY" => "ASM", "DRAWING" => "DRW", _ => "PRT" })).strong().size(16.0));
                    egui::Grid::new("creo_info").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                        ui.label("Datei:"); ui.label(info.path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()); ui.end_row();
                        ui.label("Typ:"); ui.label(match info.kind.as_str() { "PART" => "Teil", "ASSEMBLY" => "Baugruppe", "DRAWING" => "Zeichnung", k => k }); ui.end_row();
                        if !info.version.is_empty() { ui.label("Erstellt mit:"); ui.label(format!("Creo {}", info.version)); ui.end_row(); }
                        if !info.machine.is_empty() { ui.label("System:"); ui.label(&info.machine); ui.end_row(); }
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new("Die Geometrie in Creo-Dateien ist in einem geschlossenen, komprimierten Format von PTC gespeichert, das außerhalb von Creo nicht zuverlässig gelesen werden kann.").color(TEXT));
                    ui.add_space(6.0);
                    ui.label(RichText::new("So kommt das Modell zu Freo:").strong());
                    ui.label("1.  In Creo: Datei ▸ Speichern unter ▸ Kopie speichern");
                    ui.label("2.  Typ: STEP (*.stp) wählen, OK");
                    ui.label("3.  Die .stp-Datei hier öffnen – Flächen, Bohrungen und Rundungen kommen exakt an.");
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("  Schließen  ").min_size(vec2(100.0, 28.0))).clicked() { keep = false; }
                    if ui.add(egui::Button::new("STEP-Datei öffnen...").min_size(vec2(150.0, 28.0))).clicked() { pick_step = true; }
                    if let Some(sp) = &info.step {
                        let n = sp.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                        if ui.add(egui::Button::new(RichText::new(format!("{} öffnen", n)).strong()).fill(Color32::from_rgb(200, 232, 250)).min_size(vec2(150.0, 28.0))).clicked() { open_step = Some(sp.clone()); }
                    }
                });
            });
        });
        if let Some(p) = open_step { if self.confirm_discard() { self.load(p); } return; }
        if pick_step { self.browser_dir = info.path.parent().map(Path::to_path_buf).unwrap_or_else(|| self.browser_dir.clone()); self.open_in(Some(OpenKind::Step)); return; }
        if keep && x_open && !ctx.input(|i| i.key_pressed(egui::Key::Escape)) { self.creo.creo_file = Some(info); }
    }

    // ---------------- Kopie speichern / Exportieren ----------------

    pub fn export_dialog(&mut self, format: Option<super::export::Format>) {
        if self.busy() { self.say("Vor dem Exportieren das aktive KE abschließen.", MsgKind::Warning); return; }
        if self.drawing_active() { self.say("Zeichnungen über Datei ▸ PDF / DXF / SVG exportieren.", MsgKind::Info); return; }
        let dir = self.save_dir(self.path.as_ref());
        let fmt = format.or(Some(super::export::Format::StlBinary));
        self.creo.export = Some(ExportDialog { dir, name: self.data.name.to_lowercase(), format: fmt, history: vec![], selected: None });
    }

    fn export_ext(&self, f: Option<super::export::Format>) -> &'static str { match f { Some(x) => x.ext(), None => "pfprt" } }

    pub fn export_dialog_ui(&mut self, ctx: &egui::Context) {
        use super::export::Format;
        let Some(mut d) = self.creo.export.take() else { return };
        let (mut keep, mut go) = (true, false);
        let mut fs_act: Option<FsAction> = None;
        let fx = self.creo.fs.clone();
        let folders = self.common_folders();
        let asm = self.in_asm();
        let max_h = (ctx.screen_rect().height() - 200.0).clamp(240.0, 330.0);
        let mut x_open = true;
        egui::Window::new("Kopie speichern").title_bar(false).collapsible(false).resizable(false).fixed_size([880.0, max_h + 170.0]).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            if dialog_header(ui, "Kopie speichern") { x_open = false; }
            ui.horizontal(|ui| {
                if super::ribbon::icon_button(ui, Icon::Undo, "Zurück", !d.history.is_empty(), false, 14.0).clicked() { if let Some(b) = d.history.pop() { d.dir = b; } }
                if super::ribbon::icon_button(ui, Icon::Up, "Übergeordneter Ordner", d.dir.parent().is_some(), false, 14.0).clicked() {
                    if let Some(p) = d.dir.parent().map(Path::to_path_buf) { d.history.push(d.dir.clone()); d.dir = p; }
                }
                egui::Frame::NONE.fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, BORDER)).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                    ui.set_width(ui.available_width() - 8.0);
                    egui::ScrollArea::horizontal().id_salt("crumbs").stick_to_right(true).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let comps: Vec<PathBuf> = d.dir.ancestors().map(Path::to_path_buf).collect::<Vec<_>>().into_iter().rev().collect();
                        for c in comps.iter().skip(comps.len().saturating_sub(5)) {
                            let n = c.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Macintosh HD".into());
                            if ui.add(egui::Button::new(RichText::new(n).size(12.5)).frame(false)).clicked() { d.history.push(d.dir.clone()); d.dir = c.clone(); }
                            ui.label(RichText::new("▸").size(10.0).color(LABEL));
                        }
                    }));
                });
            });
            ui.horizontal(|ui| { ui.menu_button("Organisieren ▾", |ui| { fs_menu(ui, &d.dir, d.selected.as_deref(), &fx, &mut fs_act); }); });
            ui.separator();
            let ext = self.export_ext(d.format);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(200.0);
                    ui.set_min_height(max_h);
                    ui.label(RichText::new("▾  Gemeinsame Ordner").strong().size(13.0));
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (icon, label, path) in &folders {
                        let (r, _) = Self::paint_row(ui, 1, *icon, label, if path.is_some() { TEXT } else { DIM_TEXT }, false, path.as_ref() == Some(&d.dir), None);
                        if r.clicked() { if let Some(p) = path { d.history.push(d.dir.clone()); d.dir = p.clone(); } }
                    }
                });
                ui.add_space(6.0);
                egui::Frame::NONE.fill(Color32::from_rgb(245, 250, 253)).stroke(Stroke::new(1.0_f32, BORDER)).show(ui, |ui| {
                    ui.set_min_size(vec2(ui.available_width(), max_h));
                    let mut entries: Vec<(String, bool, PathBuf)> = std::fs::read_dir(&d.dir).map(|rd| rd.filter_map(|e| e.ok()).map(|e| {
                        let p = e.path();
                        (e.file_name().to_string_lossy().to_string(), p.is_dir(), p)
                    }).filter(|(n, dir, _)| !n.starts_with('.') && (*dir || n.to_lowercase().ends_with(&format!(".{}", ext)))).collect()).unwrap_or_default();
                    entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
                    egui::ScrollArea::vertical().id_salt("export_list").auto_shrink([false, false]).max_height(max_h).show(ui, |ui| {
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (name, is_dir, path) in entries {
                            let (r, _) = Self::paint_row(ui, 0, if is_dir { Icon::FolderItem } else { Icon::Export }, &name, TEXT, false, d.selected.as_ref() == Some(&path), None);
                            if r.clicked() || r.secondary_clicked() { d.selected = Some(path.clone()); }
                            if r.clicked() && !is_dir { d.name = name.rsplit_once('.').map(|x| x.0.to_string()).unwrap_or(name.clone()); }
                            if r.double_clicked() && is_dir { d.history.push(d.dir.clone()); d.dir = path.clone(); d.selected = None; }
                            r.context_menu(|ui| fs_menu(ui, &d.dir, Some(&path), &fx, &mut fs_act));
                        }
                        let h = ui.available_height();
                        let rest = vec2(ui.available_width(), if h.is_finite() { h.clamp(40.0, 2000.0) } else { 120.0 });
                        let bg = ui.allocate_response(rest, Sense::click());
                        super::ui::no_hand(ui.ctx(), bg.id);
                        bg.context_menu(|ui| fs_menu(ui, &d.dir, None, &fx, &mut fs_act));
                        });
                    });
                });
            });
            ui.add_space(6.0);
            egui::Grid::new("export_names").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Modellname:");
                ui.label(RichText::new(format!("{}.{}", self.data.name, if asm { "ASM" } else { "PRT" })).strong());
                ui.end_row();
                ui.label("Neuer Dateiname:");
                ui.horizontal(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(300.0));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { go = true; }
                    ui.label(RichText::new(format!(".{}", ext)).color(LABEL));
                });
                ui.end_row();
                ui.label("Typ:");
                ui.horizontal(|ui| {
                    let label = |f: Option<Format>| match f { Some(x) => format!("{} (*.{})", x.name(), x.ext()), None => "Freo-Teil (*.pfprt)".to_string() };
                    egui::ComboBox::from_id_salt("export_type").width(300.0).selected_text(label(d.format)).show_ui(ui, |ui| {
                        if !asm { ui.selectable_value(&mut d.format, None, label(None)); }
                        for f in Format::ALL { ui.selectable_value(&mut d.format, Some(f), label(Some(f))).on_hover_text(f.hint()); }
                    });
                    if let Some(f) = d.format { ui.label(RichText::new(f.hint()).size(11.5).color(LABEL)); }
                });
                ui.end_row();
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("  Abbrechen  ").min_size(vec2(100.0, 28.0))).clicked() { keep = false; }
                    if ui.add_enabled(!d.name.trim().is_empty(), egui::Button::new(RichText::new("OK").strong()).min_size(vec2(100.0, 28.0)).fill(Color32::from_rgb(200, 232, 250))).clicked() { go = true; }
                });
            });
        });
        if !ctx.wants_keyboard_input() && ctx.input(|i| i.modifiers.command && (i.key_pressed(egui::Key::Backspace) || i.key_pressed(egui::Key::Delete))) {
            if let Some(p) = d.selected.clone().filter(|p| p.exists()) { fs_act = Some(FsAction::Trash(p)); }
        }
        if fs_act.is_some() { self.creo.fs_action = fs_act; }
        if go && !d.name.trim().is_empty() {
            let ext = self.export_ext(d.format);
            let mut file = d.name.trim().to_string();
            if !file.to_lowercase().ends_with(&format!(".{}", ext)) { file = format!("{}.{}", file, ext); }
            let path = d.dir.join(file);
            let ok = !path.exists() || rfd::MessageDialog::new().set_title("Kopie speichern").set_description(format!("{} existiert bereits. Ersetzen?", path.display())).set_buttons(rfd::MessageButtons::YesNo).show() == rfd::MessageDialogResult::Yes;
            if ok {
                let res = match d.format {
                    Some(f) => super::export::write(&self.result, &self.data, f, &path),
                    None => serde_json::to_string_pretty(&self.data).map_err(|e| e.to_string()).and_then(|s| std::fs::write(&path, s).map_err(|e| e.to_string())),
                };
                match res {
                    Ok(_) => { self.say(format!("Kopie gespeichert: {}", path.display()), MsgKind::Success); keep = false; }
                    Err(e) => self.say(format!("Export fehlgeschlagen: {}", e), MsgKind::Error),
                }
            }
        }
        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.creo.name_dialog.is_none();
        if keep && x_open && !esc { self.creo.export = Some(d); }
    }

    // ---------------- Werkzeuge dialogs ----------------

    pub fn tool_windows(&mut self, ctx: &egui::Context) {
        self.annot_manager(ctx);
        self.model_grid_dialog(ctx);
        self.annot3_windows(ctx);
        self.file_task_dialog(ctx);
        self.regen_manager(ctx);
        self.interface_dialog(ctx);
        if let Some(mut q) = self.creo.search.take() {
            let mut open = true;
            egui::Window::new("Suchwerkzeug").open(&mut open).collapsible(false).default_width(320.0).show(ctx, |ui| {
                ui.horizontal(|ui| { ui.label("Suchen nach"); ui.label(RichText::new("KE").strong()); });
                ui.add(egui::TextEdit::singleline(&mut q).hint_text("Name enthält...").desired_width(290.0)).request_focus();
                ui.separator();
                let hits: Vec<(u64, String)> = self.data.features.iter().filter(|f| !q.is_empty() && f.name.to_lowercase().contains(&q.to_lowercase())).map(|f| (f.id, f.name.clone())).collect();
                ui.label(RichText::new(format!("{} Element(e) gefunden", hits.len())).color(LABEL));
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    for (id, n) in &hits {
                        if ui.selectable_label(self.tree_selection.contains(id), n).clicked() { self.tree_selection = vec![*id]; }
                    }
                });
                if ui.add_enabled(!hits.is_empty(), egui::Button::new("Alle auswählen")).clicked() { self.tree_selection = hits.iter().map(|h| h.0).collect(); }
            });
            if open { self.creo.search = Some(q); }
        }
        if self.creo.ref_viewer {
            let mut open = true;
            let sel = self.tree_selection.first().copied();
            egui::Window::new("Referenz-Viewer").open(&mut open).collapsible(false).default_width(420.0).show(ctx, |ui| {
                let Some(f) = sel.and_then(|id| self.data.feature(id)).cloned() else { ui.label("Ein KE im Modellbaum wählen."); return; };
                let parents: Vec<String> = f.kind.references().iter().filter_map(|id| self.data.feature(*id)).map(|p| p.name.clone()).collect();
                let children: Vec<String> = self.data.features.iter().filter(|c| c.kind.references().contains(&f.id)).map(|c| c.name.clone()).collect();
                ui.columns(3, |c| {
                    c[0].label(RichText::new("Eltern").strong());
                    if parents.is_empty() { c[0].label(RichText::new("—").color(LABEL)); }
                    for p in &parents { c[0].label(p); }
                    c[1].vertical_centered(|ui| { ui.add_space(14.0); ui.label(RichText::new(&f.name).strong().size(14.0)); ui.label(RichText::new(f.kind.type_name()).color(LABEL)); });
                    c[2].label(RichText::new("Kinder").strong());
                    if children.is_empty() { c[2].label(RichText::new("—").color(LABEL)); }
                    for k in &children { c[2].label(k); }
                });
            });
            self.creo.ref_viewer = open;
        }
        if self.creo.player {
            let mut open = true;
            let n = self.data.features.len();
            egui::Window::new("Modell-Player").open(&mut open).collapsible(false).resizable(false).default_width(360.0).show(ctx, |ui| {
                let mut pos = self.data.insert_index.unwrap_or(n);
                let name = pos.checked_sub(1).and_then(|i| self.data.features.get(i)).map(|f| f.name.clone()).unwrap_or_default();
                ui.label(format!("Regeneriert bis KE {} von {}: {}", pos, n, name));
                let mut changed = ui.add(egui::Slider::new(&mut pos, 1..=n.max(1)).show_value(false)).changed();
                ui.horizontal(|ui| {
                    if ui.button("⏮").on_hover_text("Zum Anfang").clicked() { pos = 4.min(n); changed = true; }
                    if ui.button("◀").on_hover_text("Ein KE zurück").clicked() { pos = pos.saturating_sub(1).max(1); changed = true; }
                    if ui.button("▶").on_hover_text("Ein KE vor").clicked() { pos = (pos + 1).min(n); changed = true; }
                    if ui.button("⏭").on_hover_text("Zum Ende").clicked() { pos = n; changed = true; }
                });
                if changed && self.editor.is_none() && self.sketch.is_none() {
                    self.data.insert_index = if pos >= n { None } else { Some(pos) };
                    self.regenerate(false);
                }
            });
            self.creo.player = open;
        }
    }
}

/// Navigator header strip: model tree and folder browser tabs on Creo's grey band.
pub fn nav_header(ui: &mut egui::Ui, app: &mut App) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
    ui.painter().rect_filled(r.expand2(vec2(4.0, 0.0)), 0.0, TITLE_BG);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(vec2(2.0, 3.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.spacing_mut().item_spacing.x = 4.0;
    let home = app.creo.home;
    if super::ribbon::icon_button(&mut child, Icon::TreeTab, "Modellbaum", !home, app.nav_tab == NavTab::Tree && !home, 20.0).clicked() { app.nav_tab = NavTab::Tree; }
    if super::ribbon::icon_button(&mut child, Icon::FolderTab, "Ordner-Browser", true, app.nav_tab == NavTab::Browser || home, 20.0).clicked() { app.nav_tab = NavTab::Browser; }
}

/// Days since 1970-01-01 to (year, month, day) — Howard Hinnant's civil-from-days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
