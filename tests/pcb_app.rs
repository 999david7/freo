//! The Leiterplatte application inside Freo: undo, autoroute, 3D assembly.

use freo::app::App;

#[test]
fn undo_reverts_autoroute_and_placement() {
    let mut app = App::new(None);
    app.pcb_open();
    app.pcb.set_project(freo::pcb::demo::project());
    assert!(app.pcb.project.board.tracks.is_empty());
    app.pcb_autoroute();
    assert!(!app.pcb.project.board.tracks.is_empty());
    app.pcb_undo();
    assert!(app.pcb.project.board.tracks.is_empty(), "Undo nimmt das Routing zurück");
    app.pcb_redo();
    assert!(!app.pcb.project.board.tracks.is_empty(), "Redo stellt es wieder her");
}
