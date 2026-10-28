//! The pointing-hand cursor appears over buttons only.

use eframe::egui::{self, pos2, vec2, CursorIcon, Event, RawInput, Rect};
use freo::app::ui::{hand_cursor, no_hand};

/// Runs a few frames with the pointer at `p` and returns the cursor of the last one.
fn cursor_at(p: egui::Pos2, ui_fn: &dyn Fn(&mut egui::Ui)) -> CursorIcon {
    let ctx = egui::Context::default();
    let mut icon = CursorIcon::Default;
    for _ in 0..3 {
        let input = RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))), events: vec![Event::PointerMoved(p)], ..Default::default() };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| ui_fn(ui));
            hand_cursor(ctx);
        });
        icon = out.platform_output.cursor_icon;
    }
    icon
}

#[test]
fn hand_over_buttons_arrow_elsewhere() {
    let buttons = |ui: &mut egui::Ui| {
        ui.put(Rect::from_min_size(pos2(20.0, 20.0), vec2(120.0, 30.0)), egui::Button::new("Öffnen"));
        ui.add_enabled_ui(false, |ui| { ui.put(Rect::from_min_size(pos2(20.0, 80.0), vec2(120.0, 30.0)), egui::Button::new("Aus")); });
        // a custom ribbon-style button: rect + click sense
        ui.interact(Rect::from_min_size(pos2(200.0, 20.0), vec2(60.0, 60.0)), egui::Id::new("custom"), egui::Sense::click());
        // a work area that senses clicks but must keep the arrow
        let canvas = ui.interact(Rect::from_min_size(pos2(300.0, 200.0), vec2(300.0, 300.0)), egui::Id::new("canvas"), egui::Sense::click_and_drag());
        no_hand(ui.ctx(), canvas.id);
        ui.put(Rect::from_min_size(pos2(20.0, 150.0), vec2(150.0, 24.0)), egui::TextEdit::singleline(&mut String::from("abc")));
    };
    assert_eq!(cursor_at(pos2(60.0, 35.0), &buttons), CursorIcon::PointingHand, "egui button");
    assert_eq!(cursor_at(pos2(230.0, 50.0), &buttons), CursorIcon::PointingHand, "custom click widget");
    assert_eq!(cursor_at(pos2(60.0, 95.0), &buttons), CursorIcon::Default, "disabled button");
    assert_eq!(cursor_at(pos2(450.0, 350.0), &buttons), CursorIcon::Default, "work area");
    assert_eq!(cursor_at(pos2(700.0, 100.0), &buttons), CursorIcon::Default, "empty space");
    assert_eq!(cursor_at(pos2(60.0, 162.0), &buttons), CursorIcon::Text, "text field keeps the I-beam");
}
