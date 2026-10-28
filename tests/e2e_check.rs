//! End-to-end run through the app the way a user would: Startseite, new part from the
//! template, modelling, export to every format, re-import, save/reopen, import into a part,
//! assembling an imported part and Creo files next to their STEP export.

use freo::app::export::{mass_properties, write, Format};
use freo::app::App;
use freo::model::features::*;
use freo::sketch::sketch::Sketch;

fn dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("freo_e2e_{}_{}", tag, std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn add_block(app: &mut App) {
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(PlaneRef::Datum(TOP_ID));
    e.sketch = Sketch::rect(60.0, 40.0, 0.0, 0.0);
    e.depth = Dim::v(15.0);
    app.commit("block", |d| { let id = d.next_id; let name = d.next_name(&FeatureKind::Extrude(e.clone())); d.features.push(Feature { id, name, suppressed: false, hidden: false, kind: FeatureKind::Extrude(e) }); d.next_id += 1; });
}

#[test]
fn startseite_to_part_to_every_format_and_back() {
    let d = dir("roundtrip");
    let mut app = App::new(None);
    app.go_home();
    assert!(app.creo.home && app.tab == freo::app::Tab::Home);
    // Neu ▸ Teil uses the start-part template
    app.new_part_named("HALTER");
    assert!(!app.creo.home);
    let names: Vec<String> = app.data.features.iter().map(|f| f.name.clone()).collect();
    assert_eq!(names, ["BASIS", "RECHTS", "OBEN", "VORNE", "A_X", "A_Y", "A_Z"]);
    assert!(app.result.errors.is_empty(), "{:?}", app.result.errors);
    assert_eq!(app.result.axes.len(), 3, "template axes regenerate");
    add_block(&mut app);
    assert!(app.result.errors.is_empty(), "{:?}", app.result.errors);
    let vol = mass_properties(&app.result, 1.0).volume;
    assert!((vol - 36_000.0).abs() < 1e-3, "{}", vol);
    assert_eq!(app.data.features.last().unwrap().name, "Profil 1");

    // every export format writes; STEP and STL read back to the same solid
    for f in Format::ALL { write(&app.result, &app.data, f, &d.join(format!("halter.{}", f.ext()))).unwrap(); }
    for f in ["halter.stp", "halter.stl", "halter.obj"] {
        let mut b = App::new(None);
        b.load(d.join(f));
        assert!(b.data.features.iter().any(|x| matches!(x.kind, FeatureKind::Import(_))), "{}", f);
        assert!(b.result.errors.is_empty(), "{}: {:?}", f, b.result.errors);
        let v = mass_properties(&b.result, 1.0).volume;
        assert!((v - vol).abs() / vol < 1e-4, "{}: {} vs {}", f, v, vol);
        assert!(!b.dirty, "a freshly imported file is not modified");
    }

    // an imported part saves and reopens with its geometry
    let mut b = App::new(None);
    b.load(d.join("halter.stp"));
    let p = d.join("halter_import.pfprt");
    b.path = Some(p.clone());
    b.save(false);
    let mut c = App::new(None);
    c.load(p.clone());
    assert_eq!(c.data.features.len(), b.data.features.len());
    assert!((mass_properties(&c.result, 1.0).volume - vol).abs() < 1e-2);

    // Datei ▸ Schließen returns to the Startseite
    c.close_model();
    assert!(c.creo.home);
}

#[test]
fn import_into_an_existing_part_and_undo() {
    let d = dir("insert");
    let mut a = App::new(None);
    a.new_part_named("A");
    add_block(&mut a);
    write(&a.result, &a.data, Format::StlBinary, &d.join("block.stl")).unwrap();
    let mut app = App::new(None);
    app.new_part_named("B");
    add_block(&mut app);
    let n = app.data.features.len();
    app.import_into_part(&d.join("block.stl"));
    assert_eq!(app.data.features.len(), n + 1);
    assert!(app.result.errors.is_empty(), "{:?}", app.result.errors);
    assert!(app.dirty);
    app.do_undo();
    assert_eq!(app.data.features.len(), n);
}

#[test]
fn imported_part_can_be_assembled() {
    let d = dir("asm");
    let mut a = App::new(None);
    a.new_part_named("TEIL");
    add_block(&mut a);
    write(&a.result, &a.data, Format::Step, &d.join("teil.stp")).unwrap();
    let mut b = App::new(None);
    b.load(d.join("teil.stp"));
    let part = d.join("teil.pfprt");
    b.path = Some(part.clone());
    b.save(false);
    let mut app = App::new(None);
    app.new_assembly("ASM0001");
    app.assemble_path(part);
    assert!(app.placing(), "placement starts for the imported part");
    app.place_finish();
    let asm = app.asm.as_ref().unwrap();
    assert_eq!(asm.data.components.len(), 1);
    assert!(!app.result.topo.polys.is_empty());
}

#[test]
fn creo_file_offers_the_step_export_next_to_it() {
    let d = dir("creo");
    let mut a = App::new(None);
    a.new_part_named("HALTER");
    add_block(&mut a);
    write(&a.result, &a.data, Format::Step, &d.join("halter.stp")).unwrap();
    std::fs::write(d.join("halter.prt.3"), "#UGC:2 PART 2169 1860 803 1 1 15 4100 2024014 000002cc \\\n#- CMNM 00fHALTER \\\n#-END_OF_UGC_HEADER\n").unwrap();
    let mut app = App::new(None);
    app.go_home();
    app.load(d.join("halter.prt.3"));
    let info = app.creo.creo_file.as_ref().expect("Creo dialog");
    assert_eq!(info.model, "HALTER");
    assert_eq!(info.step.as_ref().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().to_string()).as_deref(), Some("halter.stp"));
    assert!(app.creo.home, "the Startseite stays until a model is really opened");
}

#[test]
fn export_dialog_and_drawings() {
    let mut app = App::new(None);
    app.work_dir = dir("exportdlg");
    app.new_part_named("X");
    add_block(&mut app);
    app.export_dialog(None);
    let e = app.creo.export.as_ref().unwrap();
    assert_eq!(e.format, Some(Format::StlBinary));
    assert_eq!(e.name, "x");
    app.creo.export = None;
    app.new_drawing(&freo::app::drawui::NewDrawing { name: "X".into(), size: freo::model::drawing::SheetSize::A3, orient: freo::model::drawing::Orientation::Landscape, template: true, projection: freo::model::drawing::Projection::Third });
    assert!(app.drawing_active());
    app.export_dialog(None);
    assert!(app.creo.export.is_none(), "drawings export from their own Datei entries");
}

#[test]
fn new_models_are_saved_into_the_working_directory() {
    let work = dir("work").join("freo-work");
    let mut app = App::new(None);
    assert!(app.work_dir.ends_with("freo-work"), "default Arbeitsverzeichnis is freo-work");
    app.work_dir = work.clone();
    // part: Speichern of a new model needs no dialog and creates the folder
    app.new_part_named("HALTER");
    add_block(&mut app);
    app.save(false);
    assert_eq!(app.path.as_deref(), Some(work.join("halter.pfprt").as_path()));
    assert!(work.join("halter.pfprt").is_file() && !app.dirty);
    // drawing of it
    app.new_drawing(&freo::app::drawui::NewDrawing { name: "HALTER".into(), size: freo::model::drawing::SheetSize::A3, orient: freo::model::drawing::Orientation::Landscape, template: true, projection: freo::model::drawing::Projection::Third });
    app.save(false);
    assert!(work.join("halter.pfdrw").is_file());
    // assembly
    app.close_drawing();
    app.new_assembly("ASM0001");
    app.save(false);
    assert!(work.join("asm0001.pfasm").is_file());
    // the export dialog starts there too
    let mut b = App::new(None);
    b.work_dir = work.clone();
    b.new_part_named("NEU");
    add_block(&mut b);
    b.export_dialog(None);
    assert_eq!(b.creo.export.as_ref().unwrap().dir, work);
}

#[test]
fn folders_can_be_created_and_renamed() {
    use freo::app::creo::{free_folder_name, valid_name, NameDialog};
    let work = dir("folders");
    let mut app = App::new(None);
    app.work_dir = work.clone();
    // Neuer Ordner, then a second one gets a free name
    assert_eq!(free_folder_name(&work), "Neuer Ordner");
    let nd = NameDialog { parent: work.clone(), name: "Projekt A".into(), rename: None, error: None, focus: false };
    let p = app.apply_name_dialog(&nd).unwrap();
    assert!(p.is_dir() && p.ends_with("Projekt A"));
    std::fs::create_dir(work.join("Neuer Ordner")).unwrap();
    assert_eq!(free_folder_name(&work), "Neuer Ordner 2");
    // a sub folder inside it, and duplicates or bad names are refused
    let sub = app.apply_name_dialog(&NameDialog { parent: p.clone(), name: "Teile".into(), rename: None, error: None, focus: false }).unwrap();
    assert!(sub.is_dir());
    assert!(app.apply_name_dialog(&NameDialog { parent: p.clone(), name: "Teile".into(), rename: None, error: None, focus: false }).is_err());
    assert!(valid_name("  ").is_err() && valid_name("a/b").is_err() && valid_name(".x").is_err() && valid_name("Bracket v2").is_ok());
    // renaming a folder keeps the open model's path pointing at its file
    app.new_part_named("HALTER");
    add_block(&mut app);
    app.path = Some(sub.join("halter.pfprt"));
    app.save(false);
    let renamed = app.apply_name_dialog(&NameDialog { parent: work.clone(), name: "Projekt B".into(), rename: Some(p.clone()), error: None, focus: false }).unwrap();
    assert!(renamed.is_dir() && !p.exists());
    assert_eq!(app.path.as_deref(), Some(work.join("Projekt B/Teile/halter.pfprt").as_path()));
    assert!(app.path.as_ref().unwrap().is_file());
}

#[test]
fn copy_cut_paste_duplicate_trash_and_put_back() {
    use freo::app::creo::{copy_name, FsAction};
    let work = dir("fsops");
    let mut app = App::new(None);
    app.work_dir = work.clone();
    let a = work.join("Projekt A");
    let b = work.join("Projekt B");
    std::fs::create_dir_all(a.join("Teile")).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    std::fs::write(a.join("Teile/halter.pfprt"), "{}").unwrap();
    // duplicate a file and a folder: "… Kopie"
    app.fs_do(FsAction::Duplicate(a.join("Teile/halter.pfprt")), None);
    assert!(a.join("Teile/halter Kopie.pfprt").is_file());
    app.fs_do(FsAction::Duplicate(a.join("Teile")), None);
    assert!(a.join("Teile Kopie/halter.pfprt").is_file());
    assert_eq!(copy_name(&a.join("Teile"), &a.join("Teile/halter.pfprt")).file_name().unwrap(), "halter Kopie 2.pfprt");
    // copy a folder into another one
    app.fs_do(FsAction::Copy(a.join("Teile")), None);
    app.fs_do(FsAction::Paste(b.clone()), None);
    assert!(b.join("Teile/halter.pfprt").is_file() && a.join("Teile").is_dir());
    // a folder cannot be pasted into itself
    app.fs_do(FsAction::Copy(a.clone()), None);
    app.fs_do(FsAction::Paste(a.join("Teile")), None);
    assert!(!a.join("Teile/Projekt A").exists());
    // cut + paste moves, and the open model follows its file
    app.new_part_named("HALTER");
    add_block(&mut app);
    app.path = Some(a.join("Teile Kopie/halter.pfprt"));
    app.save(false);
    app.fs_do(FsAction::Cut(a.join("Teile Kopie")), None);
    app.fs_do(FsAction::Paste(b.clone()), None);
    assert!(!a.join("Teile Kopie").exists() && b.join("Teile Kopie").is_dir());
    assert_eq!(app.path.as_deref(), Some(b.join("Teile Kopie/halter.pfprt").as_path()));
    assert!(app.creo.fs.clip.is_none(), "a cut is used up by pasting");
    // Papierkorb and Zurücklegen (macOS only)
    if cfg!(target_os = "macos") {
        app.browser_dir = b.join("Teile");
        app.fs_do(FsAction::Trash(b.join("Teile")), None);
        assert!(!b.join("Teile").exists());
        assert_eq!(app.browser_dir, b, "a browser showing the deleted folder moves up");
        app.fs_do(FsAction::PutBack, None);
        assert!(b.join("Teile/halter.pfprt").is_file());
        assert!(app.creo.fs.trashed.is_none());
    }
}

#[test]
fn own_explorer_for_working_directory_and_save_as() {
    use freo::app::creo::{BrowsePurpose, SaveKind};
    let root = dir("explorer");
    let projekte = root.join("Projekte");
    std::fs::create_dir_all(projekte.join("Halter")).unwrap();
    let mut app = App::new(None);
    app.work_dir = root.clone();
    // Arbeitsverzeichnis auswählen opens Freo's explorer, not a macOS panel
    app.pick_work_dir();
    let b = app.creo.browser.as_mut().expect("explorer open");
    assert_eq!(b.purpose, BrowsePurpose::WorkDir);
    assert_eq!(b.target().unwrap(), root, "OK without a selection takes the shown folder");
    b.dir = projekte.clone();
    b.selected = Some(projekte.join("Halter"));
    assert_eq!(b.target().unwrap(), projekte.join("Halter"));
    let (purpose, target) = (b.purpose.clone(), b.target().unwrap());
    app.creo.browser = None;
    app.finish_browse(purpose, target, true);
    assert_eq!(app.work_dir, projekte.join("Halter"));
    assert!(app.creo.keep_work_dir, "kept for the next start");
    // Speichern unter goes through the explorer and writes the chosen file
    app.new_part_named("HALTER");
    add_block(&mut app);
    app.save(true);
    let b = app.creo.browser.as_mut().expect("Speichern unter explorer");
    assert_eq!(b.purpose, BrowsePurpose::SaveAs(SaveKind::Part));
    assert_eq!(b.name, "halter");
    assert_eq!(b.dir, projekte.join("Halter"), "starts in the Arbeitsverzeichnis");
    b.name = "halter_v2".into();
    let t = b.target().unwrap();
    assert!(t.ends_with("halter_v2.pfprt"), "extension added: {}", t.display());
    let purpose = b.purpose.clone();
    app.creo.browser = None;
    app.finish_browse(purpose, t.clone(), false);
    assert!(t.is_file() && app.path.as_deref() == Some(t.as_path()));
    // CSV and other exports write what they carry
    app.finish_browse(BrowsePurpose::BomCsv("Pos.;Teil\n1;HALTER\n".into()), root.join("stueck.csv"), false);
    assert_eq!(std::fs::read_to_string(root.join("stueck.csv")).unwrap(), "Pos.;Teil\n1;HALTER\n");
}
