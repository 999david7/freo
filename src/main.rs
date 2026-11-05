//! Freo — Creo-style parametric 3D CAD for macOS.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};
use freo::app::App;
use std::sync::Arc;

fn style(ctx: &egui::Context) {
    let mut s = (*ctx.style()).clone();
    let mut v = egui::Visuals::light();
    v.panel_fill = Color32::from_rgb(244, 246, 248);
    v.window_fill = Color32::WHITE;
    v.extreme_bg_color = Color32::WHITE;
    v.faint_bg_color = Color32::from_rgb(246, 248, 250);
    v.window_corner_radius = CornerRadius::same(3);
    v.menu_corner_radius = CornerRadius::same(3);
    v.window_stroke = Stroke::new(1.0_f32, Color32::from_rgb(170, 178, 188));
    v.selection.bg_fill = Color32::from_rgb(204, 227, 248);
    v.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(30, 80, 160));
    let border = Color32::from_rgb(196, 203, 211);
    for (w, fill) in [(&mut v.widgets.inactive, Color32::from_rgb(248, 249, 250)), (&mut v.widgets.hovered, Color32::from_rgb(228, 240, 252)), (&mut v.widgets.active, Color32::from_rgb(204, 227, 248)), (&mut v.widgets.open, Color32::from_rgb(228, 240, 252))] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.corner_radius = CornerRadius::same(2);
        w.bg_stroke = Stroke::new(1.0_f32, border);
        w.fg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(30, 35, 41));
    }
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(130, 180, 230));
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(214, 219, 225));
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(40, 46, 54));
    s.visuals = v;
    s.spacing.item_spacing = egui::vec2(6.0, 4.0);
    s.spacing.button_padding = egui::vec2(6.0, 2.0);
    s.text_styles = [
        (TextStyle::Heading, FontId::new(15.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(12.5, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(12.5, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(10.5, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace)),
    ].into();
    ctx.set_style(s);

    // Arial reads like Creo's Windows UI font; Apple Symbols / Arial Unicode cover the glyphs.
    let mut fonts = egui::FontDefinitions::default();
    let mut add = |name: &str, path: &str, front: bool| {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(name.into(), Arc::new(egui::FontData::from_owned(bytes)));
            let fam = fonts.families.entry(FontFamily::Proportional).or_default();
            if front { fam.insert(0, name.into()); } else { fam.push(name.into()); }
            if !front { fonts.families.entry(FontFamily::Monospace).or_default().push(name.into()); }
        }
    };
    add("arial", "/System/Library/Fonts/Supplemental/Arial.ttf", true);
    add("apple-symbols", "/System/Library/Fonts/Apple Symbols.ttf", false);
    add("arial-unicode", "/System/Library/Fonts/Supplemental/Arial Unicode.ttf", false);
    ctx.set_fonts(fonts);
}

/// The app was called ParaForm before: carry its settings (recent files, folders) over once.
fn migrate_settings() {
    let Some(home) = std::env::var_os("HOME") else { return };
    let base = std::path::PathBuf::from(home).join("Library/Application Support");
    let (old, new) = (base.join("ParaForm"), base.join("Freo"));
    if old.join("app.ron").is_file() && !new.join("app.ron").exists() && std::fs::create_dir_all(&new).is_ok() {
        let _ = std::fs::copy(old.join("app.ron"), new.join("app.ron"));
    }
}

/// Dock/window icon. eframe would otherwise put its default egui logo over the bundle icon:
/// inside Freo.app the empty icon keeps the bundle's Freo.icns, elsewhere (cargo run) we
/// draw the same icon as `make_icon`.
fn app_icon() -> egui::IconData {
    let bundled = std::env::current_exe().map(|p| p.to_string_lossy().contains(".app/Contents/MacOS")).unwrap_or(false);
    if bundled { return egui::IconData::default(); }
    let size = 256;
    egui::IconData { rgba: freo::icon::render_icon(size), width: size as u32, height: size as u32 }
}

fn main() -> eframe::Result<()> {
    // `freo --mcp`: headless MCP server on stdio for AI assistants (same as the freo-mcp binary)
    if std::env::args().skip(1).any(|a| a == "--mcp") {
        if let Err(e) = freo::mcp::serve_stdio() { eprintln!("freo --mcp: {}", e); std::process::exit(1); }
        return Ok(());
    }
    migrate_settings();
    freo::macos_open::install();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Freo")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 620.0])
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_drag_and_drop(true)
            .with_icon(app_icon()),
        depth_buffer: 24,
        multisampling: 4,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Freo",
        options,
        Box::new(|cc| {
            style(&cc.egui_ctx);
            let mut app = App::new(cc.gl.as_deref());
            if let Some(st) = cc.storage {
                if let Some(d) = st.get_string("work_dir").filter(|s| !s.is_empty()).map(std::path::PathBuf::from).filter(|p| p.is_dir()) {
                    app.work_dir = d;
                    app.creo.keep_work_dir = true;
                }
                if let Some(list) = st.get_string("recent_files").and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok()) {
                    app.recent = list.into_iter().map(std::path::PathBuf::from).filter(|p| p.exists()).collect();
                }
            }
            // like Creo, start on the Startseite in the Arbeitsverzeichnis unless a demo runs
            if std::env::var("FREO_DEMO").is_err() {
                let _ = std::fs::create_dir_all(&app.work_dir);
                app.browser_dir = app.work_dir.clone();
                app.go_home();
                app.say(format!("Arbeitsverzeichnis: {}", app.work_dir.display()), freo::app::MsgKind::Info);
            }
            if let Some(path) = std::env::args().nth(1).map(std::path::PathBuf::from) {
                if path.exists() { app.load(path); }
            }
            Ok(Box::new(app))
        }),
    )
}
