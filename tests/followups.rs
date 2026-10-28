//! Deleting views with dependents, and saving a component opened from an assembly.

use freo::app::drawing::DSel;
use freo::app::drawui::NewDrawing;
use freo::app::App;
use freo::model::drawing::*;
use freo::model::features::*;
use freo::sketch::sketch::Sketch;

#[test]
fn deleting_a_parent_view_lists_its_dependents() {
    let mut m = ModelData::new_part("B");
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(PlaneRef::Datum(TOP_ID));
    e.sketch = Sketch::rect(40.0, 30.0, 0.0, 0.0);
    e.depth = Dim::v(10.0);
    m.features.push(Feature { id: 10, name: "Extrude 1".into(), suppressed: false, hidden: false, kind: FeatureKind::Extrude(e) });
    let mut app = App::new(None);
    app.data = m;
    app.regenerate(false);
    app.new_drawing(&NewDrawing { name: "D".into(), size: SheetSize::A3, orient: Orientation::Landscape, template: true, projection: Projection::Third });
    let vs = app.drw.as_ref().unwrap().data.sheets[0].views.clone();
    // the front view carries the two projections
    app.drw.as_mut().unwrap().sel = vec![DSel::View(vs[0].id)];
    let deps = app.delete_dependents();
    assert_eq!(deps.len(), 2, "{:?}", deps);
    assert!(deps.iter().all(|d| d.contains("Projektion")));
    // selecting them too, or a view without children, needs no confirmation
    app.drw.as_mut().unwrap().sel = vs[..3].iter().map(|v| DSel::View(v.id)).collect();
    assert!(app.delete_dependents().is_empty());
    app.drw.as_mut().unwrap().sel = vec![DSel::View(vs[3].id)];
    assert!(app.delete_dependents().is_empty());
    // non-view selections never ask
    app.drw.as_mut().unwrap().sel = vec![DSel::Bom];
    assert!(app.delete_dependents().is_empty());
}

#[test]
fn saving_an_opened_component_records_its_file() {
    let dir = std::env::temp_dir().join(format!("freo_follow_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut app = App::new(None);
    app.new_assembly("ASM");
    app.create_component("WIDGET");
    let id = app.editing_from_asm().map(|o| o.comp).expect("opened for editing");
    assert!(app.asm.as_ref().unwrap().data.part("WIDGET").unwrap().source.is_none(), "created inside the assembly");
    // add a feature and save the part to its own file
    app.commit("", |d| {
        let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
        e.plane = Some(PlaneRef::Datum(TOP_ID));
        e.sketch = Sketch::rect(20.0, 20.0, 0.0, 0.0);
        e.depth = Dim::v(5.0);
        d.features.push(Feature { id: 10, name: "Extrude 1".into(), suppressed: false, hidden: false, kind: FeatureKind::Extrude(e) });
    });
    let part_file = dir.join("widget.pfprt");
    app.path = Some(part_file.clone());
    app.save(false);
    let pe = app.asm.as_ref().unwrap().data.part("WIDGET").unwrap().clone();
    assert_eq!(pe.source.as_deref(), Some(part_file.as_path()));
    assert!(!pe.modified && pe.data.features.len() == 5, "the assembly's copy is current");
    app.asm_return();
    assert!(app.in_asm());
    assert_eq!(app.tree_selection, vec![id]);
    // later edits through the assembly are written back to that file on save
    app.asm_open_part(id);
    app.commit("", |d| if let FeatureKind::Extrude(e) = &mut d.features[4].kind { e.depth = Dim::v(9.0); });
    app.asm_return();
    let asm_file = dir.join("asm.pfasm");
    app.path = Some(asm_file);
    app.save(false);
    let back: ModelData = serde_json::from_str(&std::fs::read_to_string(&part_file).unwrap()).unwrap();
    assert!(matches!(&back.features[4].kind, FeatureKind::Extrude(e) if e.depth.0 == "9"));
    let _ = std::fs::remove_dir_all(&dir);
}
