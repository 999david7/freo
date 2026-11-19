//! Freehand idea pages stored as versioned .prti documents.
mod placement;
use super::*;
use super::{
    icons::Icon,
    ribbon::{self, theme},
};
use eframe::egui::{self, vec2, Color32, Sense, Stroke};
use placement::{PlacedImage, Placement};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const PAGE: [f32; 2] = [1000.0, 1400.0];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PencilStroke {
    pub points: Vec<[f32; 2]>,
    #[serde(default)]
    pub color: [u8; 3],
    #[serde(default = "pencil_width")]
    pub width: f32,
}

fn pencil_width() -> f32 {
    2.4
}

impl PencilStroke {
    fn touches(&self, at: [f32; 2], radius: f32) -> bool {
        let at = glam::Vec2::from_array(at);
        let reach_sq = (radius + self.width * 0.5).powi(2);
        if let [p] = self.points.as_slice() {
            return at.distance_squared((*p).into()) <= reach_sq;
        }
        self.points.windows(2).any(|pair| {
            let start = glam::Vec2::from_array(pair[0]);
            let delta = glam::Vec2::from_array(pair[1]) - start;
            let fraction = ((at - start).dot(delta) / delta.length_squared().max(f32::EPSILON)).clamp(0.0, 1.0);
            at.distance_squared(start + delta * fraction) <= reach_sq
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdeaFile {
    pub format: String,
    pub version: u32,
    pub name: String,
    pub strokes: Vec<PencilStroke>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    images: Vec<PlacedImage>,
}

impl IdeaFile {
    pub fn new(name: &str) -> Self {
        Self { format: "Freo Idea".into(), version: 1, name: name.into(), strokes: vec![], images: vec![] }
    }

    pub fn read(path: &std::path::Path) -> Result<Self, String> {
        let data: Self = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if data.format != "Freo Idea" || !matches!(data.version, 1 | 2) {
            return Err("Unbekanntes Idee-Dateiformat.".into());
        }
        if data.strokes.iter().any(|stroke| !stroke.width.is_finite() || !(0.1..=100.0).contains(&stroke.width)) {
            return Err("Ungültige Strichbreite.".into());
        }
        if data.strokes.iter().flat_map(|stroke| &stroke.points).any(|p| !p[0].is_finite() || !p[1].is_finite() || p[0] < 0.0 || p[0] > PAGE[0] || p[1] < 0.0 || p[1] > PAGE[1]) {
            return Err("Die Idee enthält ungültige Stiftpositionen.".into());
        }
        for image in &data.images {
            if (0..2)
                .any(|i| !image.min[i].is_finite() || !image.size[i].is_finite() || image.min[i] < -0.001 || image.size[i] <= 0.0 || image.min[i] + image.size[i] > PAGE[i] + 0.001)
            {
                return Err("Ungültige Bildposition.".into());
            }
            placement::pixels(&image.png)?;
        }
        Ok(data)
    }

    pub fn write(&self, path: &std::path::Path) -> Result<(), String> {
        self.publish(path, true).map_err(|error| error.to_string())
    }

    fn publish(&self, path: &std::path::Path, replace: bool) -> std::io::Result<()> {
        use std::io::Write;
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let temp = path.with_file_name(format!(".freo-idea-{}-{}.tmp", std::process::id(), stamp));
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
        let result = (|| {
            let mut writer = std::io::BufWriter::new(&mut file);
            serde_json::to_writer(&mut writer, self)?;
            writer.flush()?;
            drop(writer);
            file.sync_all()?;
            if replace {
                std::fs::rename(&temp, path)
            } else {
                std::fs::hard_link(&temp, path)
            }
        })();
        if !replace || result.is_err() {
            let _ = std::fs::remove_file(temp);
        }
        result
    }

    fn create_in(&self, dir: &std::path::Path) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let name = self.name.to_lowercase().replace(['/', ':', '\\', '\0'], "_");
        let stem = name.trim_matches(|c: char| c == '.' || c.is_whitespace());
        let stem = if stem.is_empty() { "idee" } else { stem };
        for number in 1u64.. {
            let filename = if number == 1 { format!("{stem}.prti") } else { format!("{stem}-{number}.prti") };
            let path = dir.join(filename);
            match self.publish(&path, false) {
                Ok(()) => return Ok(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        unreachable!()
    }
}

pub struct IdeaSession {
    pub data: IdeaFile,
    pub path: Option<PathBuf>,
    saved: IdeaFile,
    undo: Vec<IdeaFile>,
    redo: Vec<IdeaFile>,
    drawing: bool,
    erasing: bool,
    placement: Option<Placement>,
    anchor: Option<egui::Vec2>,
    palette_open: bool,
    palette_tab: usize,
    textures: Vec<(Arc<String>, egui::TextureHandle)>,
    import_error: Option<String>,
    ink: [u8; 3],
    width: f32,
    save_error: Option<String>,
}

impl IdeaSession {
    pub fn new(data: IdeaFile, path: Option<PathBuf>) -> Self {
        Self {
            saved: data.clone(),
            data,
            path,
            undo: vec![],
            redo: vec![],
            drawing: false,
            erasing: false,
            placement: None,
            anchor: None,
            palette_open: false,
            palette_tab: 0,
            textures: vec![],
            import_error: None,
            ink: [0; 3],
            width: pencil_width(),
            save_error: None,
        }
    }

    pub fn dirty(&self) -> bool {
        self.path.is_none() || self.data != self.saved
    }

    fn checkpoint(&mut self) {
        self.undo.push(self.data.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn apply_tool(&mut self, point: [f32; 2], radius: f32) {
        if self.erasing {
            if let Some(index) = self.data.strokes.iter().rposition(|stroke| stroke.touches(point, radius)) {
                if !self.drawing {
                    self.checkpoint();
                    self.drawing = true;
                }
                self.data.strokes.remove(index);
            }
        } else {
            if !self.drawing {
                self.checkpoint();
                self.data.strokes.push(PencilStroke { points: vec![], color: self.ink, width: self.width });
                self.drawing = true;
            }
            let points = &mut self.data.strokes.last_mut().unwrap().points;
            if points.last() != Some(&point) {
                points.push(point);
            }
        }
    }

    pub(super) fn import_picture(&mut self, ctx: &egui::Context, path: &std::path::Path) {
        match placement::import_image(path) {
            Ok((png, pixels)) => {
                let texture = ctx.load_texture("Ideenbild", pixels, egui::TextureOptions::LINEAR);
                self.placement = Some(Placement::Image(png, texture));
                self.anchor = None;
                self.erasing = false;
                self.import_error = None;
            }
            Err(error) => self.import_error = Some(format!("Bildimport fehlgeschlagen: {error}")),
        }
    }

    fn palette(&mut self, ctx: &egui::Context) {
        if !self.palette_open {
            return;
        }
        let tabs = crate::sketch::palette::tabs();
        let mut pick = None;
        egui::Window::new("2D-Palette").open(&mut self.palette_open).default_width(310.0).show(ctx, |ui| {
            egui::ComboBox::from_id_salt("idea_category").selected_text(tabs[self.palette_tab].name).show_ui(ui, |ui| {
                for (i, tab) in tabs.iter().enumerate() {
                    ui.selectable_value(&mut self.palette_tab, i, tab.name);
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(350.0).id_salt(self.palette_tab).show(ui, |ui| {
                for (i, item) in tabs[self.palette_tab].items.iter().enumerate() {
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 38.), Sense::click());
                    if ui.is_rect_visible(rect) {
                        ribbon::button_bg(ui, rect, &response, true, matches!(self.placement, Some(Placement::Palette(t,n)) if t == self.palette_tab && n == i));
                        super::sketcher::palette_thumbnail(ui.painter(), egui::Rect::from_min_size(rect.min, vec2(38., 38.)), item);
                        ui.painter().text(rect.left_center() + vec2(46., 0.), egui::Align2::LEFT_CENTER, &item.name, egui::FontId::proportional(14.), theme::TEXT);
                    }
                    if response.clicked() {
                        pick = Some(i);
                    }
                }
            });
        });
        if let Some(i) = pick {
            self.placement = Some(Placement::Palette(self.palette_tab, i));
            self.erasing = false;
            self.anchor = None;
            self.palette_open = false;
        }
    }

    pub fn undo(&mut self) {
        self.anchor = None;
        if let Some(strokes) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.data, strokes));
        }
        self.drawing = false;
    }

    pub fn redo(&mut self) {
        self.anchor = None;
        if let Some(strokes) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.data, strokes));
        }
        self.drawing = false;
    }
}

impl App {
    pub fn new_idea(&mut self, name: &str) {
        self.new_part_named(name);
        self.idea = Some(IdeaSession::new(IdeaFile::new(name), None));
        self.messages.clear();
        self.save_idea(false);
    }

    pub fn load_idea(&mut self, path: PathBuf) {
        match IdeaFile::read(&path) {
            Ok(data) => {
                self.new_part_named(&data.name);
                self.idea = Some(IdeaSession::new(data, Some(path.clone())));
                self.add_recent(&path);
            }
            Err(error) => self.say(format!("Idee konnte nicht geöffnet werden: {}", error), MsgKind::Error),
        }
    }

    pub fn save_idea(&mut self, save_as: bool) -> bool {
        let Some(idea) = &self.idea else { return false };
        let result = if save_as {
            let Some(path) = rfd::FileDialog::new()
                .set_title("Idee speichern")
                .add_filter("Freo Idee", &["prti"])
                .set_directory(self.save_dir(idea.path.as_ref()))
                .set_file_name(format!("{}.prti", idea.data.name.to_lowercase()))
                .save_file()
                .map(|mut path| {
                    path.set_extension("prti");
                    path
                })
            else {
                return false;
            };
            idea.data.write(&path).map(|_| path)
        } else if let Some(path) = &idea.path {
            idea.data.write(path).map(|_| path.clone())
        } else {
            idea.data.create_in(&self.work_dir).map_err(|error| error.to_string())
        };
        let idea = self.idea.as_mut().unwrap();
        match result {
            Ok(path) => {
                idea.save_error = None;
                idea.path = Some(path.clone());
                idea.saved = idea.data.clone();
                self.add_recent(&path);
                true
            }
            Err(error) => {
                idea.save_error = Some(error);
                false
            }
        }
    }

    pub fn idea_frame(&mut self, ctx: &egui::Context) {
        let mut save = false;
        let mut save_as = false;
        let mut done = false;
        let mut open = false;
        let mut new = false;
        let mut close = false;
        let idea = self.idea.as_mut().unwrap();
        egui::TopBottomPanel::top("idea_title").exact_height(34.0).frame(egui::Frame::NONE.fill(theme::TITLE_BG)).show(ctx, |ui| {
            let rect = ui.max_rect();
            let drag = ui.allocate_rect(rect, Sense::click_and_drag());
            if drag.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if drag.double_clicked() {
                let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            let brand = rect.left_center() + vec2(84.0, 0.0);
            let mark = egui::Rect::from_center_size(brand, vec2(14.0, 14.0));
            let blue = Color32::from_rgb(52, 120, 190);
            ui.painter().rect_stroke(mark, 2, Stroke::new(2.0_f32, blue), egui::StrokeKind::Inside);
            ui.painter().rect_filled(mark.shrink(4.0), 0, blue);
            ui.painter().text(brand + vec2(12.0, 0.0), egui::Align2::LEFT_CENTER, "freo", egui::FontId::proportional(17.0), theme::TEXT);
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, format!("{} – Idee", idea.data.name), egui::FontId::proportional(14.0), Color32::BLACK);
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{}{} – Idee – Freo", idea.data.name, if idea.dirty() { " *" } else { "" })));
        if self.new_dialog.is_none() && !ctx.wants_keyboard_input() {
            ctx.input(|input| {
                if input.key_pressed(egui::Key::Escape) {
                    idea.anchor = None;
                    idea.placement = None;
                    idea.palette_open = false;
                }
                if input.modifiers.command {
                    if input.key_pressed(egui::Key::Z) {
                        if input.modifiers.shift {
                            idea.redo();
                        } else {
                            idea.undo();
                        }
                    }
                    save = input.key_pressed(egui::Key::S);
                    save_as = input.modifiers.shift;
                    open = input.key_pressed(egui::Key::O);
                    new = input.key_pressed(egui::Key::N);
                    close = input.key_pressed(egui::Key::W);
                }
            });
        }
        egui::TopBottomPanel::top("idea_tabs").exact_height(32.0).frame(egui::Frame::NONE.fill(theme::TAB_BG)).show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let (file_rect, file_tab) = ui.allocate_exact_size(vec2(76.0, 32.0), Sense::click());
                ui.painter().rect_filled(file_rect, 0, theme::FILE_GREEN);
                ui.painter().text(file_rect.center(), egui::Align2::CENTER_CENTER, "Datei", egui::FontId::proportional(14.0), Color32::WHITE);
                let menu_id = file_tab.id.with("menu");
                if file_tab.clicked() {
                    ui.memory_mut(|memory| memory.toggle_popup(menu_id));
                }
                egui::popup_below_widget(ui, menu_id, &file_tab, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
                    ui.set_min_width(140.0);
                    for (label, action) in [("Neu", &mut new), ("Öffnen…", &mut open), ("Speichern", &mut save)] {
                        if ui.button(label).clicked() {
                            *action = true;
                            ui.memory_mut(|memory| memory.close_popup());
                        }
                    }
                    if ui.button("Speichern unter…").clicked() {
                        save = true;
                        save_as = true;
                        ui.memory_mut(|memory| memory.close_popup());
                    }
                });
                egui::Frame::NONE.fill(theme::RIB_TOP).inner_margin(egui::Margin::symmetric(18, 7)).show(ui, |ui| {
                    let (icon, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                    super::icons::paint(ui.painter(), icon, Icon::Idea, true);
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Idee").strong().color(theme::ACCENT));
                });
            });
        });
        egui::TopBottomPanel::top("idea_tools").exact_height(42.0).frame(egui::Frame::NONE.fill(theme::RIB_TOP).inner_margin(egui::Margin::symmetric(10, 0))).show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                if ribbon::small(ui, Icon::Sketch, "Stift", true, !idea.erasing && idea.placement.is_none()).clicked() {
                    idea.erasing = false;
                    idea.anchor = None;
                    idea.placement = None;
                }
                if ribbon::small(ui, Icon::Eraser, "Radierer", true, idea.erasing).on_hover_text("Ganze Striche entfernen").clicked() {
                    idea.erasing = true;
                    idea.anchor = None;
                    idea.placement = None;
                }
                if ribbon::small(ui, Icon::SkCircle, "Kreis", true, matches!(idea.placement, Some(Placement::Circle))).clicked() {
                    idea.placement = Some(Placement::Circle);
                    idea.anchor = None;
                    idea.erasing = false;
                }
                if ribbon::small(ui, Icon::SkPalette, "Palette", true, matches!(idea.placement, Some(Placement::Palette(..)))).clicked() {
                    idea.palette_open = true;
                }
                if ribbon::small(ui, Icon::TlImage, "Bild", true, matches!(idea.placement, Some(Placement::Image(..)))).on_hover_text("PNG- oder JPEG-Bild einfügen").clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("Bilder", &["png", "jpg", "jpeg"]).pick_file() {
                        idea.import_picture(ctx, &path);
                    }
                }
                ui.separator();
                for (name, rgb) in
                    [("Schwarz", [0, 0, 0]), ("Blau", [43, 103, 175]), ("Grün", [43, 129, 90]), ("Orange", [206, 114, 37]), ("Rot", [183, 59, 67]), ("Violett", [126, 81, 162])]
                {
                    let (rect, swatch) = ui.allocate_exact_size(vec2(21.0, 24.0), Sense::click());
                    let selected = idea.ink == rgb && !idea.erasing;
                    if swatch.on_hover_text(name).clicked() {
                        idea.ink = rgb;
                        idea.erasing = false;
                    }
                    ui.painter().circle_filled(rect.center(), 6.5, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
                    if selected {
                        ui.painter().circle_stroke(rect.center(), 9.0, Stroke::new(1.2_f32, theme::TEXT));
                    }
                }
                ui.separator();
                ui.label("Strichbreite");
                let widths = [("Fein", 1.3), ("Normal", pencil_width()), ("Breit", 6.0), ("Extra breit", 12.0)];
                egui::ComboBox::from_id_salt("idea_width").width(88.0).selected_text(widths.iter().find(|(_, w)| *w == idea.width).unwrap().0).show_ui(ui, |ui| {
                    for (label, width) in widths {
                        if ui.selectable_value(&mut idea.width, width, label).clicked() {
                            idea.erasing = false;
                        }
                    }
                });
                ui.separator();
                if ribbon::icon_button(ui, Icon::Undo, "Rückgängig", !idea.undo.is_empty(), false, 22.0).clicked() {
                    idea.undo();
                }
                if ribbon::icon_button(ui, Icon::Redo, "Wiederholen", !idea.redo.is_empty(), false, 22.0).clicked() {
                    idea.redo();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    done = ribbon::small(ui, Icon::Ok, "Fertig", true, false).clicked();
                });
            });
        });
        egui::TopBottomPanel::bottom("idea_status").exact_height(28.0).frame(egui::Frame::NONE.fill(theme::STATUS_BG).inner_margin(egui::Margin::symmetric(12, 0))).show(
            ctx,
            |ui| {
                ui.horizontal_centered(|ui| {
                    if let Some(error) = &idea.import_error {
                        ui.colored_label(Color32::DARK_RED, error);
                        if ui.small_button("Schließen").clicked() {
                            idea.import_error = None;
                        }
                    } else if let Some(error) = &idea.save_error {
                        ui.colored_label(Color32::DARK_RED, format!("Speichern fehlgeschlagen: {error}"));
                        save |= ui.small_button("Erneut versuchen").clicked();
                    } else if let Some(tool) = &idea.placement {
                        ui.label(match tool {
                            Placement::Circle => "Kreis: Vom Mittelpunkt zum Rand ziehen.",
                            Placement::Palette(t, i) => &crate::sketch::palette::tabs()[*t].items[*i].name,
                            Placement::Image(..) => "Bild",
                        });
                        if !matches!(tool, Placement::Circle) {
                            ui.label("Klicken zum Platzieren oder ziehen, um die Größe festzulegen.");
                        }
                        ui.weak("Esc beendet das Werkzeug");
                    } else {
                        let pending = idea.dirty();
                        let (dot, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                        ui.painter().circle_filled(dot.center(), 3.0, if pending { theme::DATUM } else { theme::ACCENT });
                        ui.label(if pending { "Änderungen werden gespeichert…" } else { "Automatisch gespeichert" });
                        if let Some(path) = &idea.path {
                            ui.separator();
                            ui.label(path.file_name().unwrap_or_default().to_string_lossy()).on_hover_text(path.display().to_string());
                        }
                    }
                });
            },
        );
        idea.palette(ctx);
        let modal = self.new_dialog.is_some() || idea.palette_open;
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
            let desk = ui.available_rect_before_wrap();
            super::ui::graphics_background_bg(ui.painter(), desk, self.creo.bg);
            let available = desk.shrink(20.0);
            let scale = (available.width() / PAGE[0]).min(available.height() / PAGE[1]).max(0.01);
            let page = egui::Rect::from_center_size(available.center(), vec2(PAGE[0] * scale, PAGE[1] * scale));
            let response = ui.allocate_rect(page, Sense::click_and_drag());
            ui.painter().rect_filled(page.translate(vec2(2.0, 4.0)), 1.0, Color32::from_black_alpha(42));
            ui.painter().rect_filled(page, 0.0, Color32::WHITE);
            let painter = ui.painter().with_clip_rect(page);
            if response.hovered() {
                ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            let screen_rect = |min: [f32; 2], size: [f32; 2]| egui::Rect::from_min_size(page.min + vec2(min[0], min[1]) * scale, vec2(size[0], size[1]) * scale);
            let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.));
            idea.textures.retain(|(png, _)| idea.data.images.iter().any(|image| Arc::ptr_eq(&image.png, png)));
            for image in &idea.data.images {
                let texture = idea.textures.iter().find(|(png, _)| Arc::ptr_eq(&image.png, png));
                let id = if let Some((_, texture)) = texture {
                    texture.id()
                } else {
                    let Ok(pixels) = placement::pixels(&image.png) else { continue };
                    let tex = ctx.load_texture("Ideenbild", pixels, egui::TextureOptions::LINEAR);
                    let id = tex.id();
                    idea.textures.push((image.png.clone(), tex));
                    id
                };
                painter.image(id, screen_rect(image.min, image.size), uv, Color32::WHITE);
            }
            let placing = idea.placement.is_some();
            let mut preview = Vec::new();
            if !modal && idea.placement.is_some() {
                let local = |p: egui::Pos2| ((p - page.min) / scale).clamp(egui::Vec2::ZERO, vec2(PAGE[0], PAGE[1]));
                if response.is_pointer_button_down_on() || response.clicked() {
                    if let Some(start) = ctx.input(|i| i.pointer.press_origin()).filter(|p| page.contains(*p)) {
                        idea.anchor.get_or_insert(local(start));
                    }
                }
                if let Some(end) = ctx.input(|i| i.pointer.latest_pos()).filter(|p| idea.anchor.is_some() || response.hovered() && page.contains(*p)) {
                    let at = idea.anchor.unwrap_or_else(|| local(end));
                    let radius = (local(end) - at).length();
                    let released = idea.anchor.is_some() && ctx.input(|i| i.pointer.primary_released());
                    match idea.placement.as_ref().unwrap() {
                        Placement::Circle if radius > 0.5 => {
                            preview.push(placement::circle(at, radius));
                        }
                        Placement::Circle => {}
                        Placement::Palette(t, i) => {
                            preview = placement::palette_paths(&crate::sketch::palette::tabs()[*t].items[*i], at, if radius < 2. { 160. } else { 2. * radius });
                        }
                        Placement::Image(png, tex) => {
                            let size = tex.size_vec2();
                            let bounds = placement::fit(at, size / size.max_elem(), if radius < 2. { 720. } else { 2. * radius });
                            let min = [bounds.min.x, bounds.min.y];
                            let size = [bounds.width(), bounds.height()];
                            painter.image(tex.id(), screen_rect(min, size), uv, Color32::WHITE);
                            if released {
                                let image = PlacedImage { png: png.clone(), min, size };
                                let cached = (png.clone(), tex.clone());
                                idea.checkpoint();
                                idea.data.version = 2;
                                idea.data.images.push(image);
                                idea.textures.push(cached);
                                idea.placement = None;
                            }
                        }
                    }
                    if released {
                        if !preview.is_empty() {
                            idea.checkpoint();
                            idea.data.strokes.extend(preview.drain(..).map(|points| PencilStroke { points, width: idea.width, color: idea.ink }));
                        }
                        idea.anchor = None;
                        ctx.request_repaint();
                    }
                }
            }
            let point = response.interact_pointer_pos().filter(|p| page.contains(*p));
            if !modal && !placing && (response.is_pointer_button_down_on() || response.clicked()) && point.is_some() {
                let p = (point.unwrap() - page.min) / scale;
                let point = [p.x.clamp(0.0, PAGE[0]), p.y.clamp(0.0, PAGE[1])];
                idea.apply_tool(point, 8.0 / scale);
                ctx.request_repaint();
            } else {
                idea.drawing = false;
            }
            for stroke in &idea.data.strokes {
                let color = Color32::from_rgb(stroke.color[0], stroke.color[1], stroke.color[2]);
                let width = (stroke.width * scale).max(0.6);
                let points: Vec<_> = stroke.points.iter().map(|p| page.min + vec2(p[0], p[1]) * scale).collect();
                if points.len() == 1 {
                    painter.circle_filled(points[0], width * 0.5, color);
                } else if points.len() > 1 {
                    painter.add(egui::Shape::line(points, Stroke::new(width, color)));
                }
            }
            for path in preview {
                let points = path.iter().map(|p| page.min + vec2(p[0], p[1]) * scale).collect();
                painter.add(egui::Shape::line(points, Stroke::new((idea.width * scale).max(0.6), Color32::from_rgb(idea.ink[0], idea.ink[1], idea.ink[2]))));
            }
            if idea.erasing && response.hovered() {
                if let Some(at) = response.hover_pos() {
                    painter.circle_stroke(at, 8.0, Stroke::new(1.0_f32, theme::TEXT));
                }
            }
        });
        let autosave = !idea.drawing && idea.dirty() && idea.save_error.is_none();
        if save || done || autosave {
            let saved = self.save_idea(save && save_as);
            ctx.request_repaint();
            if done && saved {
                self.go_home();
            }
        }
        if close && self.confirm_discard() {
            self.go_home();
        }
        if new {
            self.confirm_new();
        }
        if open && self.confirm_discard() {
            if let Some(path) = rfd::FileDialog::new().set_title("Idee öffnen").add_filter("Freo Idee", &["prti"]).pick_file() {
                self.load_idea(path);
            }
        }
        self.new_dialogs(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idea_file_round_trip_and_failed_load_keep_work() {
        let path = std::env::temp_dir().join(format!("freo-idea-test-{}.prti", std::process::id()));
        let mut data = IdeaFile::new("KONZEPT");
        data.strokes.push(PencilStroke { points: vec![[10.0, 20.0], [30.0, 40.0]], width: 6.0, color: [43, 103, 175] });
        data.write(&path).unwrap();
        assert_eq!(IdeaFile::read(&path).unwrap(), data);
        let mut app = App::new(None);
        app.load(path.clone());
        assert_eq!(app.idea.as_ref().unwrap().data, data);
        assert!(!app.idea.as_ref().unwrap().dirty());
        std::fs::write(&path, br#"{"format":"Other","version":1,"name":"bad","strokes":[]}"#).unwrap();
        app.load(path.clone());
        assert_eq!(app.idea.as_ref().unwrap().data, data);
        app.new_part();
        assert!(app.idea.is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn place_image_circle_and_library_profile_then_reopen() {
        let dir = std::env::temp_dir().join(format!("freo-idea-placement-{}", std::process::id()));
        let mut app = App::new(None);
        app.work_dir = dir.clone();
        app.new_idea("Formen");
        let source = dir.join("reference.jpg");
        image::RgbImage::from_pixel(80, 40, image::Rgb([25, 75, 150])).save(&source).unwrap();
        let ctx = egui::Context::default();
        app.idea.as_mut().unwrap().import_picture(&ctx, &source);
        std::fs::remove_file(source).unwrap();
        let frame = |app: &mut App, events| {
            let _ = ctx
                .run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1000., 800.))), events, ..Default::default() }, |ctx| app.idea_frame(ctx));
        };
        let place = |app: &mut App, start, end| {
            let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            frame(app, vec![]);
            frame(app, vec![egui::Event::PointerMoved(start), button(start, true)]);
            frame(app, vec![egui::Event::PointerMoved(end)]);
            frame(app, vec![button(end, false)]);
        };
        let start = egui::pos2(500., 350.);
        let end = egui::pos2(562., 385.);
        place(&mut app, start, start);
        let idea = app.idea.as_mut().unwrap();
        let picture = &idea.data.images[0];
        assert_eq!(picture.size[0], 2. * picture.size[1]);
        assert!(idea.data.strokes.is_empty());
        idea.placement = Some(Placement::Circle);
        place(&mut app, start, end);
        let idea = app.idea.as_mut().unwrap();
        let circle = &idea.data.strokes[0];
        assert_eq!(circle.points.first(), circle.points.last());
        assert_eq!(circle.points.len(), 129);
        idea.placement = Some(Placement::Palette(0, 0));
        place(&mut app, start, end);
        let idea = app.idea.as_mut().unwrap();
        let path = idea.path.clone().unwrap();
        let saved = IdeaFile::read(&path).unwrap();
        assert_eq!(idea.data, saved);
        assert!(saved.strokes.len() > 1);
        idea.undo();
        assert_eq!(idea.data.strokes.len(), 1);
        idea.undo();
        idea.undo();
        assert!(idea.data.images.is_empty());
        app.load_idea(path.clone());
        assert_eq!(app.idea.as_ref().unwrap().data, saved);
        frame(&mut app, vec![]);
        assert_eq!(app.idea.as_ref().unwrap().textures.len(), 1);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn legacy_ink_and_eraser_history() {
        let legacy: PencilStroke = serde_json::from_str(r#"{"points":[[0,0],[100,0]]}"#).unwrap();
        assert_eq!((legacy.width, legacy.color), (pencil_width(), [0; 3]));
        let mut idea = IdeaSession::new(IdeaFile::new("Test"), None);
        idea.data.strokes.push(legacy.clone());
        idea.width = 6.0;
        idea.ink = [35, 100, 180];
        idea.apply_tool([50., 30.], 2.);
        assert_eq!(idea.data.strokes[1].color, idea.ink);
        assert_eq!(idea.data.strokes[1].width, idea.width);
        idea.drawing = false;
        idea.erasing = true;
        idea.apply_tool([50., 15.], 2.);
        assert_eq!(idea.data.strokes.len(), 2);
        idea.apply_tool([50., 1.], 2.);
        idea.apply_tool([50., 30.], 2.);
        assert!(idea.data.strokes.is_empty());
        idea.undo();
        assert_eq!(idea.data.strokes.len(), 2);
        assert_eq!(idea.data.strokes[0], legacy);
        idea.redo();
        assert!(idea.data.strokes.is_empty());
    }

    #[test]
    fn pencil_draws_and_undo_redo_restore_the_page() {
        let ctx = egui::Context::default();
        let mut app = App::new(None);
        let dir = std::env::temp_dir().join(format!("freo-idea-autosave-{}", std::process::id()));
        app.work_dir = dir.clone();
        app.new_idea("TEST");
        let original = app.idea.as_ref().unwrap().path.clone().unwrap();
        app.new_idea("TEST");
        assert_ne!(Some(&original), app.idea.as_ref().unwrap().path.as_ref());
        let mut frame = |events| {
            let _ = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 800.0))), events, ..Default::default() }, |ctx| {
                app.idea_frame(ctx)
            });
        };
        frame(vec![]);
        let start = egui::pos2(500.0, 300.0);
        frame(vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
        ]);
        let end = egui::pos2(550.0, 350.0);
        frame(vec![egui::Event::PointerMoved(end)]);
        frame(vec![egui::Event::PointerButton { pos: end, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE }]);
        frame(vec![]);
        let idea = app.idea.as_mut().unwrap();
        assert_eq!(idea.data.strokes.len(), 1);
        assert!(idea.data.strokes[0].points.len() >= 2);
        assert!(!idea.dirty());
        assert_eq!(IdeaFile::read(idea.path.as_ref().unwrap()).unwrap(), idea.data);
        let drawn = idea.data.clone();
        idea.undo();
        assert!(idea.dirty());
        assert!(idea.data.strokes.is_empty());
        let path = idea.path.clone().unwrap();
        let _ = ctx.run(Default::default(), |ctx| app.idea_frame(ctx));
        assert!(IdeaFile::read(&path).unwrap().strokes.is_empty());
        app.idea.as_mut().unwrap().redo();
        let _ = ctx.run(Default::default(), |ctx| app.idea_frame(ctx));
        assert_eq!(IdeaFile::read(&path).unwrap(), drawn);
        assert!(IdeaFile::read(&original).unwrap().strokes.is_empty());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(original).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
